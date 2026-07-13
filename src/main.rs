use std::ffi::OsString;
use std::process::ExitCode;

use cc_switchy::config::ConfigStore;
use cc_switchy::{
    commands, AppError, AppPaths, Cli, Language, MessageArgs, MessageKey, RunMode, Translator,
};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> ExitCode {
    let raw_args = std::env::args_os().collect::<Vec<_>>();
    let discovered_paths = AppPaths::discover();
    let persisted_language = discovered_paths
        .as_ref()
        .ok()
        .and_then(|paths| ConfigStore::new(paths.config_file.clone()).load().ok())
        .map(|config| config.language);
    let language = Language::resolve(language_override(&raw_args), persisted_language);
    let translator = Translator::new(language);
    let cli = match Cli::parse_localized_from(raw_args, &translator) {
        Ok(cli) => cli,
        Err(error) => {
            let output = Cli::localized_error(&error, &translator);
            if error.use_stderr() {
                eprint!("{output}");
            } else {
                print!("{output}");
            }
            return ExitCode::from(error.exit_code() as u8);
        }
    };

    let result = match discovered_paths {
        Ok(paths) => dispatch(cli.run_mode(), paths, &translator).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(exit_code) => exit_code,
        Err(AppError::NoSourceConfigured) => {
            let args = MessageArgs::default();
            eprintln!(
                "{}\n{}",
                translator.text(MessageKey::NoSourceConfigured, &args),
                translator.text(MessageKey::RunWizard, &args)
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            let args = MessageArgs::default();
            let exit_code = 1;
            let detail = match &error {
                AppError::HomeDirectoryUnavailable => {
                    translator.text(MessageKey::HomeDirectoryUnavailable, &args)
                }
                AppError::SyncLocked => translator.text(MessageKey::ErrorSyncLocked, &args),
                AppError::Cancelled => translator.text(MessageKey::ErrorCancelled, &args),
                AppError::NoSourceConfigured => unreachable!("handled above"),
                other => other.to_string(),
            };
            eprintln!(
                "{}: {detail}",
                translator.text(MessageKey::UnexpectedError, &args)
            );
            ExitCode::from(exit_code)
        }
    }
}

async fn dispatch(
    mode: RunMode,
    paths: AppPaths,
    translator: &Translator,
) -> Result<ExitCode, AppError> {
    match mode {
        RunMode::Sync { source } => {
            let cancellation = CancellationToken::new();
            let signal = cancellation.clone();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    signal.cancel();
                }
            });
            let outcome = commands::run_cli(paths, source, translator, cancellation).await?;
            if outcome.projection.warnings.is_empty() {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(2))
            }
        }
        RunMode::Tui { source } => commands::tui::run(paths, translator.language(), source)
            .await
            .map(|()| ExitCode::SUCCESS),
        RunMode::Wizard => commands::wizard::run(paths, translator.language())
            .await
            .map(|()| ExitCode::SUCCESS),
    }
}

fn language_override(args: &[OsString]) -> Option<&str> {
    args.iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, argument)| {
            let argument = argument.to_str()?;
            if argument == "--lang" {
                args.get(index + 1)?.to_str()
            } else {
                argument.strip_prefix("--lang=")
            }
        })
}
