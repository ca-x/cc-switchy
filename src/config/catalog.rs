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
        self.store.save(&candidate)?;
        self.config = candidate;
        Ok(())
    }
}
