use super::{AppConfig, ConfigStore, SourceConfig};
use crate::{AppError, Language};

pub struct SourceCatalog {
    store: ConfigStore,
    config: AppConfig,
}

impl SourceCatalog {
    pub fn load(store: ConfigStore) -> Result<Self, AppError> {
        let config = store.load()?;
        Ok(Self { store, config })
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn add(&mut self, mut source: SourceConfig) -> Result<(), AppError> {
        source.normalize();
        source.validate()?;
        if self
            .config
            .sources
            .iter()
            .any(|item| item.name == source.name)
        {
            return Err(AppError::DuplicateSource(source.name));
        }

        let mut candidate = self.config.clone();
        if candidate.default_source.is_none() {
            candidate.default_source = Some(source.name.clone());
        }
        candidate.sources.push(source);
        self.commit(candidate)
    }

    pub fn update(
        &mut self,
        original_name: &str,
        mut source: SourceConfig,
    ) -> Result<(), AppError> {
        let original_name = original_name.trim();
        source.normalize();
        source.validate()?;
        let index = self
            .config
            .sources
            .iter()
            .position(|item| item.name == original_name)
            .ok_or_else(|| AppError::SourceNotFound(original_name.to_string()))?;
        if self
            .config
            .sources
            .iter()
            .enumerate()
            .any(|(candidate, item)| candidate != index && item.name == source.name)
        {
            return Err(AppError::DuplicateSource(source.name));
        }

        let mut candidate = self.config.clone();
        candidate.sources[index] = source.clone();
        if candidate.default_source.as_deref() == Some(original_name) {
            candidate.default_source = Some(source.name);
        }
        self.commit(candidate)
    }

    pub fn delete(
        &mut self,
        name: &str,
        replacement_default: Option<&str>,
    ) -> Result<(), AppError> {
        let name = name.trim();
        if !self.config.sources.iter().any(|source| source.name == name) {
            return Err(AppError::SourceNotFound(name.to_string()));
        }

        let mut candidate = self.config.clone();
        candidate.sources.retain(|source| source.name != name);
        if candidate.default_source.as_deref() == Some(name) {
            if candidate.sources.is_empty() {
                candidate.default_source = None;
            } else {
                let replacement = replacement_default
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or(AppError::ReplacementDefaultRequired)?;
                if !candidate
                    .sources
                    .iter()
                    .any(|source| source.name == replacement)
                {
                    return Err(AppError::SourceNotFound(replacement.to_string()));
                }
                candidate.default_source = Some(replacement.to_string());
            }
        }
        self.commit(candidate)
    }

    pub fn set_default(&mut self, name: &str) -> Result<(), AppError> {
        let name = name.trim();
        if !self.config.sources.iter().any(|source| source.name == name) {
            return Err(AppError::SourceNotFound(name.to_string()));
        }
        let mut candidate = self.config.clone();
        candidate.default_source = Some(name.to_string());
        self.commit(candidate)
    }

    pub fn set_language(&mut self, language: Language) -> Result<(), AppError> {
        let mut candidate = self.config.clone();
        candidate.language = language;
        self.commit(candidate)
    }

    pub fn resolve(&self, explicit: Option<&str>) -> Result<&SourceConfig, AppError> {
        let name = explicit
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .or(self.config.default_source.as_deref())
            .ok_or(AppError::NoSourceConfigured)?;
        self.config
            .sources
            .iter()
            .find(|source| source.name == name)
            .ok_or_else(|| AppError::SourceNotFound(name.to_string()))
    }

    fn commit(&mut self, mut candidate: AppConfig) -> Result<(), AppError> {
        candidate.normalize();
        candidate.validate()?;
        match self.store.save(&candidate) {
            Ok(()) => {
                self.config = candidate;
                Ok(())
            }
            Err(error) => match self.store.load() {
                Ok(persisted) if persisted == candidate => {
                    self.config = persisted;
                    Ok(())
                }
                _ => Err(error),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::config::{SourceKind, WebDavConfig};

    #[test]
    fn committed_add_is_reconciled_after_a_post_persist_error() {
        let home = TempDir::new().expect("home");
        let path = home.path().join("config.toml");
        let store = ConfigStore::failing_after_persist(path.clone());
        let mut catalog = SourceCatalog::load(store).expect("catalog");

        catalog.add(webdav("home")).expect("reconciled add");

        assert_eq!(catalog.config().default_source.as_deref(), Some("home"));
        let reloaded = SourceCatalog::load(ConfigStore::new(path)).expect("reloaded catalog");
        assert_eq!(reloaded.config().sources[0].name, "home");
    }

    #[test]
    fn committed_rename_is_reconciled_after_a_post_persist_error() {
        let home = TempDir::new().expect("home");
        let path = home.path().join("config.toml");
        let mut seed = SourceCatalog::load(ConfigStore::new(path.clone())).expect("seed catalog");
        seed.add(webdav("old")).expect("seed source");

        let store = ConfigStore::failing_after_persist(path.clone());
        let mut catalog = SourceCatalog::load(store).expect("catalog");
        catalog
            .update("old", webdav("new"))
            .expect("reconciled rename");

        assert_eq!(catalog.config().default_source.as_deref(), Some("new"));
        let reloaded = SourceCatalog::load(ConfigStore::new(path)).expect("reloaded catalog");
        assert_eq!(reloaded.config().sources[0].name, "new");
    }

    #[test]
    fn failed_write_is_not_reconciled_when_disk_does_not_match() {
        let home = TempDir::new().expect("home");
        let blocking_parent = home.path().join("not-a-directory");
        std::fs::write(&blocking_parent, "blocking file").expect("blocking file");
        let path = blocking_parent.join("config.toml");
        let mut catalog = SourceCatalog::load(ConfigStore::new(path)).expect("default catalog");

        assert!(catalog.add(webdav("home")).is_err());
        assert!(catalog.config().sources.is_empty());
    }

    fn webdav(name: &str) -> SourceConfig {
        SourceConfig {
            name: name.to_string(),
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            kind: SourceKind::WebDav {
                webdav: WebDavConfig {
                    base_url: "https://dav.example.test".to_string(),
                    username: "user".to_string(),
                    password: "secret".to_string(),
                },
            },
        }
    }
}
