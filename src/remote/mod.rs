pub mod protocol;
pub mod webdav;

#[derive(Debug)]
pub struct DownloadedSnapshot {
    pub manifest: protocol::ValidatedManifest,
    pub manifest_bytes: Vec<u8>,
    pub layout: protocol::RemoteLayout,
    pub db_sql_path: std::path::PathBuf,
    pub skills_zip_path: std::path::PathBuf,
}
