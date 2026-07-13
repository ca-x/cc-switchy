use std::ffi::OsString;
use std::process::ExitCode;

use cc_switchy::{AppError, AppPaths, Cli, Language, MessageArgs, MessageKey, RunMode, Translator};

fn main() -> ExitCode {
    let raw_args = std::env::args_os().collect::<Vec<_>>();
    let language = Language::resolve(language_override(&raw_args), None);
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

    match AppPaths::discover().and_then(|paths| dispatch(cli.run_mode(), &paths)) {
        Ok(()) => ExitCode::SUCCESS,
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
            let detail = match error {
                AppError::HomeDirectoryUnavailable => {
                    translator.text(MessageKey::HomeDirectoryUnavailable, &args)
                }
                AppError::NoSourceConfigured => unreachable!("handled above"),
                other => other.to_string(),
            };
            eprintln!(
                "{}: {detail}",
                translator.text(MessageKey::UnexpectedError, &args)
            );
            ExitCode::FAILURE
        }
    }
}

fn dispatch(_mode: RunMode, _paths: &AppPaths) -> Result<(), AppError> {
    Err(AppError::NoSourceConfigured)
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
