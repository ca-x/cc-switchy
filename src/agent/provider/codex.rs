//! Codex atomic live projection adapted from CC Switch (MIT).

use std::fs;
use std::path::PathBuf;

use std::collections::HashSet;

use serde_json::{json, Map, Value};
use toml_edit::DocumentMut;

use super::{atomic_write, with_file_rollback, write_json};
use crate::agent::{Agent, AgentPaths};
use crate::AppError;

pub(crate) fn managed_paths(paths: &AgentPaths) -> Result<Vec<PathBuf>, AppError> {
    let directory = paths.config_dir(Agent::Codex)?;
    Ok(vec![
        directory.join("auth.json"),
        directory.join("config.toml"),
        directory.join("cc-switch-model-catalog.json"),
    ])
}

pub(crate) fn write(paths: &AgentPaths, settings: &Value) -> Result<(), AppError> {
    let object = settings.as_object().ok_or_else(|| {
        AppError::Restore("Codex provider settings must be a JSON object".to_string())
    })?;
    let auth = object
        .get("auth")
        .ok_or_else(|| AppError::Restore("Codex provider settings are missing auth".to_string()))?;
    if !auth.is_object() {
        return Err(AppError::Restore(
            "Codex provider auth must be a JSON object".to_string(),
        ));
    }
    let raw_config = object.get("config").and_then(Value::as_str).unwrap_or("");
    if !raw_config.trim().is_empty() {
        raw_config
            .parse::<DocumentMut>()
            .map_err(|error| AppError::Restore(format!("invalid Codex config.toml: {error}")))?;
    }
    let (config, catalog) = prepare_model_catalog(settings, raw_config)?;

    let files = managed_paths(paths)?;
    with_file_rollback(&files, || {
        write_json(&files[0], auth)?;
        atomic_write(&files[1], config.as_bytes()).and_then(|()| match &catalog {
            Some(catalog) => write_json(&files[2], catalog),
            None => Ok(()),
        })
    })
}

pub(crate) fn read(paths: &AgentPaths) -> Result<Option<Value>, AppError> {
    let files = managed_paths(paths)?;
    if !files.iter().any(|path| path.is_file()) {
        return Ok(None);
    }
    let auth = if files[0].is_file() {
        let bytes = fs::read(&files[0]).map_err(|error| AppError::io(&files[0], error))?;
        serde_json::from_slice(&bytes).map_err(|error| {
            AppError::Restore(format!("invalid {}: {error}", files[0].display()))
        })?
    } else {
        json!({})
    };
    let config = if files[1].is_file() {
        fs::read_to_string(&files[1]).map_err(|error| AppError::io(&files[1], error))?
    } else {
        String::new()
    };
    Ok(Some(json!({"auth": auth, "config": config})))
}

fn prepare_model_catalog(
    settings: &Value,
    config: &str,
) -> Result<(String, Option<Value>), AppError> {
    let mut document = if config.trim().is_empty() {
        DocumentMut::new()
    } else {
        config
            .parse::<DocumentMut>()
            .map_err(|error| AppError::Restore(format!("invalid Codex config.toml: {error}")))?
    };
    let models = settings
        .pointer("/modelCatalog/models")
        .and_then(Value::as_array);
    let Some(models) = models.filter(|models| !models.is_empty()) else {
        let owns_pointer = document
            .get("model_catalog_json")
            .and_then(|item| item.as_str())
            .and_then(|path| std::path::Path::new(path).file_name())
            .and_then(|name| name.to_str())
            == Some("cc-switch-model-catalog.json");
        if owns_pointer {
            document.as_table_mut().remove("model_catalog_json");
        }
        return Ok((document.to_string(), None));
    };

    let template: Value = serde_json::from_str(include_str!(
        "../../../resources/codex_native_responses_template.json"
    ))
    .map_err(|error| AppError::Restore(format!("invalid bundled Codex template: {error}")))?;
    let default_context = document
        .get("model_context_window")
        .and_then(|item| item.as_integer())
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(128_000);
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for (priority, model) in models.iter().enumerate() {
        let Some(slug) = model
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|slug| !slug.is_empty())
        else {
            continue;
        };
        if !seen.insert(slug.to_string()) {
            continue;
        }
        let display_name = model
            .get("displayName")
            .or_else(|| model.get("display_name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(slug);
        let context_window = positive_u64(
            model
                .get("contextWindow")
                .or_else(|| model.get("context_window")),
        )
        .unwrap_or(default_context);
        let mut entry = template.clone();
        let entry = entry.as_object_mut().ok_or_else(|| {
            AppError::Restore("bundled Codex template must be an object".to_string())
        })?;
        update_catalog_entry(entry, model, slug, display_name, context_window, priority);
        entries.push(Value::Object(entry.clone()));
    }

    if entries.is_empty() {
        return Ok((document.to_string(), None));
    }
    document["model_catalog_json"] = toml_edit::value("cc-switch-model-catalog.json");
    Ok((document.to_string(), Some(json!({"models": entries}))))
}

fn update_catalog_entry(
    entry: &mut Map<String, Value>,
    model: &Value,
    slug: &str,
    display_name: &str,
    context_window: u64,
    priority: usize,
) {
    entry.insert("slug".to_string(), json!(slug));
    entry.insert("display_name".to_string(), json!(display_name));
    entry.insert("description".to_string(), json!(display_name));
    entry.insert("context_window".to_string(), json!(context_window));
    entry.insert("max_context_window".to_string(), json!(context_window));
    entry.insert("priority".to_string(), json!(1000 + priority));
    entry.insert("additional_speed_tiers".to_string(), json!([]));
    entry.insert("service_tiers".to_string(), json!([]));
    entry.insert("availability_nux".to_string(), Value::Null);
    entry.insert("upgrade".to_string(), Value::Null);
    for key in [
        "apply_patch_tool_type",
        "web_search_tool_type",
        "tools",
        "model_messages",
    ] {
        entry.remove(key);
    }
    entry.insert("shell_type".to_string(), json!("shell_command"));
    if let Some(value) = model
        .get("supportsParallelToolCalls")
        .or_else(|| model.get("supports_parallel_tool_calls"))
        .and_then(Value::as_bool)
    {
        entry.insert("supports_parallel_tool_calls".to_string(), json!(value));
    }
    if let Some(value) = model
        .get("inputModalities")
        .or_else(|| model.get("input_modalities"))
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
    {
        entry.insert("input_modalities".to_string(), Value::Array(value.clone()));
    }
    if let Some(value) = model
        .get("baseInstructions")
        .or_else(|| model.get("base_instructions"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        entry.insert("base_instructions".to_string(), json!(value));
    }
}

fn positive_u64(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(value)) => value.as_u64().filter(|value| *value > 0),
        Some(Value::String(value)) => value.trim().parse().ok().filter(|value| *value > 0),
        _ => None,
    }
}
