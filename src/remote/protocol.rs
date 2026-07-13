//! CC Switch v2 download protocol compatibility.
//!
//! This module adapts the transport-independent manifest validation from
//! CC Switch. It deliberately contains no upload behavior.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AppError;

pub const PROTOCOL_FORMAT: &str = "cc-switch-webdav-sync";
pub const PROTOCOL_VERSION: u32 = 2;
pub const DB_COMPAT_VERSION: u32 = 6;
pub const LEGACY_DB_COMPAT_VERSION: u32 = 5;
pub const REMOTE_DB_SQL: &str = "db.sql";
pub const REMOTE_SKILLS_ZIP: &str = "skills.zip";
pub const REMOTE_MANIFEST: &str = "manifest.json";
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_SYNC_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncManifest {
    pub format: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_compat_version: Option<u32>,
    pub device_name: String,
    pub created_at: String,
    pub artifacts: BTreeMap<String, ArtifactMeta>,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactMeta {
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteLayout {
    Current,
    Legacy,
}

impl RemoteLayout {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedManifest {
    pub manifest: SyncManifest,
    db_compat_version: u32,
    layout: RemoteLayout,
}

impl ValidatedManifest {
    pub fn db_compat_version(&self) -> u32 {
        self.db_compat_version
    }

    pub fn layout(&self) -> RemoteLayout {
        self.layout
    }

    pub fn artifact(&self, name: &str) -> Result<&ArtifactMeta, AppError> {
        self.manifest
            .artifacts
            .get(name)
            .ok_or_else(|| AppError::ManifestMissingArtifact {
                artifact: name.to_string(),
            })
    }

    pub fn snapshot_id(&self) -> &str {
        &self.manifest.snapshot_id
    }
}

impl SyncManifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, AppError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(AppError::ManifestTooLarge {
                size: bytes.len(),
                max: MAX_MANIFEST_BYTES,
            });
        }
        serde_json::from_slice(bytes).map_err(AppError::ManifestParse)
    }

    pub fn validate(self, layout: RemoteLayout) -> Result<ValidatedManifest, AppError> {
        if self.format != PROTOCOL_FORMAT {
            return Err(AppError::ManifestFormatIncompatible { found: self.format });
        }
        if self.version != PROTOCOL_VERSION {
            return Err(AppError::ManifestVersionIncompatible {
                found: self.version,
                supported: PROTOCOL_VERSION,
            });
        }

        let db_compat_version = self
            .db_compat_version
            .or_else(|| (layout == RemoteLayout::Legacy).then_some(LEGACY_DB_COMPAT_VERSION))
            .ok_or(AppError::DatabaseVersionMissing)?;
        let incompatible = match layout {
            RemoteLayout::Current => db_compat_version != DB_COMPAT_VERSION,
            RemoteLayout::Legacy => db_compat_version > DB_COMPAT_VERSION,
        };
        if incompatible {
            return Err(AppError::DatabaseVersionIncompatible {
                found: db_compat_version,
                supported: DB_COMPAT_VERSION,
            });
        }

        for artifact in [REMOTE_DB_SQL, REMOTE_SKILLS_ZIP] {
            let meta =
                self.artifacts
                    .get(artifact)
                    .ok_or_else(|| AppError::ManifestMissingArtifact {
                        artifact: artifact.to_string(),
                    })?;
            validate_artifact_meta(artifact, meta)?;
        }

        let expected_snapshot_id = compute_snapshot_id(&self.artifacts);
        if !self.snapshot_id.eq_ignore_ascii_case(&expected_snapshot_id) {
            return Err(AppError::SnapshotIdMismatch {
                expected: expected_snapshot_id,
                actual: self.snapshot_id,
            });
        }

        Ok(ValidatedManifest {
            manifest: self,
            db_compat_version,
            layout,
        })
    }
}

pub fn compute_snapshot_id(artifacts: &BTreeMap<String, ArtifactMeta>) -> String {
    let parts = artifacts
        .iter()
        .map(|(name, meta)| format!("{name}:{}", meta.sha256.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    sha256_hex(parts.join("|").as_bytes())
}

pub fn verify_artifact(
    bytes: &[u8],
    artifact_name: &str,
    meta: &ArtifactMeta,
) -> Result<(), AppError> {
    if bytes.len() as u64 != meta.size {
        return Err(AppError::ArtifactSizeMismatch {
            artifact: artifact_name.to_string(),
            expected: meta.size,
            actual: bytes.len() as u64,
        });
    }

    let actual = sha256_hex(bytes);
    if !actual.eq_ignore_ascii_case(&meta.sha256) {
        return Err(AppError::ArtifactHashMismatch {
            artifact: artifact_name.to_string(),
            expected: meta.sha256.clone(),
            actual,
        });
    }
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn validate_artifact_meta(name: &str, meta: &ArtifactMeta) -> Result<(), AppError> {
    if meta.size > MAX_SYNC_ARTIFACT_BYTES {
        return Err(AppError::ArtifactTooLarge {
            artifact: name.to_string(),
            size: meta.size,
            max: MAX_SYNC_ARTIFACT_BYTES,
        });
    }
    if meta.sha256.len() != 64 || !meta.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::InvalidArtifactHash {
            artifact: name.to_string(),
        });
    }
    Ok(())
}
