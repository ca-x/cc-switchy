use std::collections::BTreeMap;

use cc_switchy::progress::{ChannelProgress, ProgressEvent, ProgressSink};
use cc_switchy::remote::protocol::{
    compute_snapshot_id, sha256_hex, verify_artifact, ArtifactMeta, RemoteLayout, SyncManifest,
    DB_COMPAT_VERSION, MAX_MANIFEST_BYTES, MAX_SYNC_ARTIFACT_BYTES, REMOTE_DB_SQL,
    REMOTE_SKILLS_ZIP,
};
use cc_switchy::AppError;

fn fixture() -> Vec<u8> {
    include_bytes!("fixtures/cc-switch-v2/manifest.json").to_vec()
}

fn manifest() -> SyncManifest {
    SyncManifest::parse(&fixture()).expect("fixture manifest")
}

#[test]
fn current_manifest_parses_and_validates() {
    let validated = manifest()
        .validate(RemoteLayout::Current)
        .expect("valid current manifest");

    assert_eq!(validated.db_compat_version(), DB_COMPAT_VERSION);
    assert_eq!(validated.artifact(REMOTE_DB_SQL).unwrap().size, 123);
    assert_eq!(validated.artifact(REMOTE_SKILLS_ZIP).unwrap().size, 456);
    assert_eq!(validated.layout(), RemoteLayout::Current);
}

#[test]
fn oversized_manifest_is_rejected_before_json_parsing() {
    let oversized = vec![b' '; MAX_MANIFEST_BYTES + 1];

    assert!(matches!(
        SyncManifest::parse(&oversized),
        Err(AppError::ManifestTooLarge { .. })
    ));
}

#[test]
fn incompatible_format_protocol_and_current_db_version_are_rejected() {
    let mut wrong_format = manifest();
    wrong_format.format = "unknown".to_string();
    assert!(matches!(
        wrong_format.validate(RemoteLayout::Current),
        Err(AppError::ManifestFormatIncompatible { .. })
    ));

    let mut wrong_protocol = manifest();
    wrong_protocol.version = 3;
    assert!(matches!(
        wrong_protocol.validate(RemoteLayout::Current),
        Err(AppError::ManifestVersionIncompatible { .. })
    ));

    let mut wrong_db = manifest();
    wrong_db.db_compat_version = Some(5);
    assert!(matches!(
        wrong_db.validate(RemoteLayout::Current),
        Err(AppError::DatabaseVersionIncompatible { .. })
    ));
}

#[test]
fn legacy_manifest_without_db_version_uses_db_v5() {
    let mut legacy = manifest();
    legacy.db_compat_version = None;

    let validated = legacy
        .validate(RemoteLayout::Legacy)
        .expect("legacy manifest");

    assert_eq!(validated.db_compat_version(), 5);
    assert_eq!(validated.layout(), RemoteLayout::Legacy);
}

#[test]
fn current_manifest_requires_an_explicit_db_version() {
    let mut current = manifest();
    current.db_compat_version = None;

    assert!(matches!(
        current.validate(RemoteLayout::Current),
        Err(AppError::DatabaseVersionMissing)
    ));
}

#[test]
fn legacy_manifest_cannot_exceed_the_supported_db_version() {
    let mut legacy = manifest();
    legacy.db_compat_version = Some(DB_COMPAT_VERSION + 1);

    assert!(matches!(
        legacy.validate(RemoteLayout::Legacy),
        Err(AppError::DatabaseVersionIncompatible { .. })
    ));
}

#[test]
fn required_artifacts_and_size_limits_are_enforced() {
    let mut missing = manifest();
    missing.artifacts.remove(REMOTE_SKILLS_ZIP);
    missing.snapshot_id = compute_snapshot_id(&missing.artifacts);
    assert!(matches!(
        missing.validate(RemoteLayout::Current),
        Err(AppError::ManifestMissingArtifact { .. })
    ));

    let mut oversized = manifest();
    oversized.artifacts.get_mut(REMOTE_DB_SQL).unwrap().size = MAX_SYNC_ARTIFACT_BYTES + 1;
    oversized.snapshot_id = compute_snapshot_id(&oversized.artifacts);
    assert!(matches!(
        oversized.validate(RemoteLayout::Current),
        Err(AppError::ArtifactTooLarge { .. })
    ));
}

#[test]
fn snapshot_id_must_match_the_sorted_artifact_hashes() {
    let mut mismatched = manifest();
    mismatched.snapshot_id = "0".repeat(64);

    assert!(matches!(
        mismatched.validate(RemoteLayout::Current),
        Err(AppError::SnapshotIdMismatch { .. })
    ));
}

#[test]
fn artifact_hash_metadata_must_be_sha256_hex() {
    let mut invalid = manifest();
    invalid.artifacts.get_mut(REMOTE_DB_SQL).unwrap().sha256 = "not-a-hash".to_string();
    invalid.snapshot_id = compute_snapshot_id(&invalid.artifacts);

    assert!(matches!(
        invalid.validate(RemoteLayout::Current),
        Err(AppError::InvalidArtifactHash { .. })
    ));
}

#[test]
fn artifact_bytes_must_match_size_and_sha256() {
    let bytes = b"verified bytes";
    let meta = ArtifactMeta {
        sha256: sha256_hex(bytes),
        size: bytes.len() as u64,
    };
    verify_artifact(bytes, "db.sql", &meta).expect("valid artifact");

    let wrong_size = ArtifactMeta {
        size: meta.size + 1,
        ..meta.clone()
    };
    assert!(matches!(
        verify_artifact(bytes, "db.sql", &wrong_size),
        Err(AppError::ArtifactSizeMismatch { .. })
    ));

    let wrong_hash = ArtifactMeta {
        sha256: "f".repeat(64),
        ..meta
    };
    assert!(matches!(
        verify_artifact(bytes, "db.sql", &wrong_hash),
        Err(AppError::ArtifactHashMismatch { .. })
    ));
}

#[test]
fn snapshot_id_is_deterministic_for_sorted_artifacts() {
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        "skills.zip".to_string(),
        ArtifactMeta {
            sha256: "b".repeat(64),
            size: 2,
        },
    );
    artifacts.insert(
        "db.sql".to_string(),
        ArtifactMeta {
            sha256: "a".repeat(64),
            size: 1,
        },
    );

    assert_eq!(
        compute_snapshot_id(&artifacts),
        "924931de8535d599eeef1a42ecf8a676a84ce80c5a41e9684089f6e784d2bb30"
    );
}

#[test]
fn channel_progress_preserves_byte_counts_without_credentials() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let progress = ChannelProgress::new(sender);
    progress.emit(ProgressEvent::Downloading {
        artifact: "db.sql".to_string(),
        downloaded: 25,
        total: 100,
    });

    let event = receiver.recv().expect("progress event");
    assert_eq!(
        event,
        ProgressEvent::Downloading {
            artifact: "db.sql".to_string(),
            downloaded: 25,
            total: 100,
        }
    );
    let debug = format!("{event:?}");
    assert!(!debug.contains("password"));
    assert!(!debug.contains("secret"));
}
