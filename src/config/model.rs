use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{AppError, Language};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub version: u32,
    #[serde(default)]
    pub language: Language,
    #[serde(default)]
    pub default_source: Option<String>,
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            language: Language::Auto,
            default_source: None,
            sources: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn normalize(&mut self) {
        self.default_source = self
            .default_source
            .take()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty());
        for source in &mut self.sources {
            source.normalize();
        }
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.version != CONFIG_VERSION {
            return Err(AppError::InvalidConfig(format!(
                "unsupported config version {}",
                self.version
            )));
        }

        let mut names = HashSet::with_capacity(self.sources.len());
        for source in &self.sources {
            source.validate()?;
            if !names.insert(source.name.as_str()) {
                return Err(AppError::DuplicateSource(source.name.clone()));
            }
        }

        match (&self.default_source, self.sources.is_empty()) {
            (None, true) => Ok(()),
            (Some(_), true) => Err(AppError::InvalidConfig(
                "default source is set while the source list is empty".to_string(),
            )),
            (None, false) => Err(AppError::InvalidConfig(
                "a default source is required".to_string(),
            )),
            (Some(default), false) if names.contains(default.as_str()) => Ok(()),
            (Some(default), false) => Err(AppError::SourceNotFound(default.clone())),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceConfig {
    pub name: String,
    #[serde(default = "default_remote_root")]
    pub remote_root: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(flatten)]
    pub kind: SourceKind,
}

impl SourceConfig {
    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.remote_root = self.remote_root.trim().to_string();
        self.profile = self.profile.trim().to_string();
        if self.remote_root.is_empty() {
            self.remote_root = default_remote_root();
        }
        if self.profile.is_empty() {
            self.profile = default_profile();
        }

        match &mut self.kind {
            SourceKind::WebDav { webdav } => {
                webdav.base_url = webdav.base_url.trim().to_string();
                webdav.username = webdav.username.trim().to_string();
            }
            SourceKind::S3 { s3 } => {
                s3.region = s3.region.trim().to_string();
                s3.bucket = s3.bucket.trim().to_string();
                s3.endpoint = s3.endpoint.trim().to_string();
                s3.access_key_id = s3.access_key_id.trim().to_string();
            }
        }
    }

    pub fn validate(&self) -> Result<(), AppError> {
        require_value("source name", &self.name)?;
        require_value("remote root", &self.remote_root)?;
        require_value("profile", &self.profile)?;

        match &self.kind {
            SourceKind::WebDav { webdav } => webdav.validate(),
            SourceKind::S3 { s3 } => s3.validate(),
        }
    }

    pub fn redacted(&self) -> RedactedSource<'_> {
        RedactedSource(self)
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SourceKind {
    WebDav { webdav: WebDavConfig },
    S3 { s3: S3Config },
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDavConfig {
    pub base_url: String,
    pub username: String,
    pub password: String,
}

impl WebDavConfig {
    fn validate(&self) -> Result<(), AppError> {
        require_value("WebDAV URL", &self.base_url)?;
        require_value("WebDAV username", &self.username)?;
        require_value("WebDAV password", &self.password)?;
        let url = Url::parse(&self.base_url)
            .map_err(|_| AppError::InvalidConfig("WebDAV URL is invalid".to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(AppError::InvalidConfig(
                "WebDAV URL must use HTTP or HTTPS".to_string(),
            ));
        }
        if url.host_str().is_none() {
            return Err(AppError::InvalidConfig(
                "WebDAV URL must include a host".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct S3Config {
    pub region: String,
    pub bucket: String,
    #[serde(default)]
    pub endpoint: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl S3Config {
    fn validate(&self) -> Result<(), AppError> {
        require_value("S3 region", &self.region)?;
        require_value("S3 bucket", &self.bucket)?;
        require_value("S3 access key ID", &self.access_key_id)?;
        require_value("S3 secret access key", &self.secret_access_key)?;
        if !self.endpoint.is_empty() {
            let endpoint = if self.endpoint.contains("://") {
                self.endpoint.clone()
            } else {
                format!("https://{}", self.endpoint)
            };
            let url = Url::parse(&endpoint)
                .map_err(|_| AppError::InvalidConfig("S3 endpoint is invalid".to_string()))?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err(AppError::InvalidConfig(
                    "S3 endpoint must be an HTTP or HTTPS host".to_string(),
                ));
            }
        }
        Ok(())
    }
}

pub struct RedactedSource<'a>(&'a SourceConfig);

impl fmt::Debug for RedactedSource<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("SourceConfig");
        debug
            .field("name", &self.0.name)
            .field("remote_root", &self.0.remote_root)
            .field("profile", &self.0.profile);
        match &self.0.kind {
            SourceKind::WebDav { webdav } => debug
                .field("type", &"webdav")
                .field("endpoint", &safe_endpoint(&webdav.base_url)),
            SourceKind::S3 { s3 } => debug
                .field("type", &"s3")
                .field("bucket", &s3.bucket)
                .field("region", &s3.region)
                .field("endpoint", &safe_endpoint(&s3.endpoint))
                .field("access_key_id", &mask_access_key_id(&s3.access_key_id)),
        };
        debug.finish()
    }
}

fn require_value(label: &str, value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        Err(AppError::InvalidConfig(format!("{label} is required")))
    } else {
        Ok(())
    }
}

fn safe_endpoint(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let candidate = if value.contains("://") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let Ok(mut url) = Url::parse(&candidate) else {
        return "<invalid endpoint>".to_string();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn mask_access_key_id(value: &str) -> String {
    let characters = value.chars().collect::<Vec<_>>();
    if characters.len() <= 4 {
        return "****".to_string();
    }
    let visible = characters.len().min(4);
    let prefix = characters[..visible].iter().collect::<String>();
    format!("{prefix}{}", "*".repeat(characters.len() - visible))
}

fn default_remote_root() -> String {
    "cc-switch-sync".to_string()
}

fn default_profile() -> String {
    "default".to_string()
}
