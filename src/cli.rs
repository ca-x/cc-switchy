use clap::error::{ContextKind, ErrorKind};
use clap::{Command, CommandFactory, FromArgMatches, Parser};

use crate::i18n::{MessageArgs, MessageKey, Translator};

#[derive(Parser, Debug)]
#[command(name = "cc-switchy", version, disable_help_subcommand = true)]
pub struct Cli {
    #[arg(long, conflicts_with = "sync")]
    pub wizard: bool,
    #[arg(long, conflicts_with = "wizard")]
    pub sync: bool,
    #[arg(long)]
    pub source: Option<String>,
    #[arg(long, value_parser = ["zh", "en"])]
    pub lang: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunMode {
    Tui { source: Option<String> },
    Wizard,
    Sync { source: Option<String> },
}

impl Cli {
    pub fn localized_command(translator: &Translator) -> Command {
        let args = MessageArgs::default();
        let usage_heading = translator.text(MessageKey::HelpUsage, &args);
        let options_heading = translator.text(MessageKey::HelpOptions, &args);
        let usage = match translator.language() {
            crate::i18n::Language::ZhCn => "cc-switchy [选项]",
            crate::i18n::Language::Auto | crate::i18n::Language::EnUs => "cc-switchy [OPTIONS]",
        };
        let (source_value_name, language_value_name) = match translator.language() {
            crate::i18n::Language::ZhCn => ("来源", "语言"),
            crate::i18n::Language::Auto | crate::i18n::Language::EnUs => ("SOURCE", "LANGUAGE"),
        };
        let help_template = format!(
            "{{about-with-newline}}\n{usage_heading}: {{usage}}\n\n{options_heading}:\n{{options}}"
        );

        let mut command = <Self as CommandFactory>::command();
        command.build();

        command
            .about(translator.text(MessageKey::HelpAbout, &args))
            .override_usage(usage)
            .help_template(help_template)
            .mut_arg("wizard", |arg| {
                arg.help(translator.text(MessageKey::HelpWizard, &args))
            })
            .mut_arg("sync", |arg| {
                arg.help(translator.text(MessageKey::HelpSync, &args))
            })
            .mut_arg("source", |arg| {
                arg.value_name(source_value_name)
                    .help(translator.text(MessageKey::HelpSource, &args))
            })
            .mut_arg("lang", |arg| {
                arg.value_name(language_value_name)
                    .hide_possible_values(true)
                    .help(translator.text(MessageKey::HelpLanguage, &args))
            })
            .mut_arg("help", |arg| {
                arg.help(translator.text(MessageKey::HelpFlag, &args))
            })
            .mut_arg("version", |arg| {
                arg.help(translator.text(MessageKey::VersionFlag, &args))
            })
    }

    pub fn parse_localized_from<I, T>(args: I, translator: &Translator) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let matches = Self::localized_command(translator).try_get_matches_from(args)?;
        Self::from_arg_matches(&matches)
    }

    pub fn localized_error(error: &clap::Error, translator: &Translator) -> String {
        if !error.use_stderr() {
            return error.to_string();
        }

        let mut args = MessageArgs::default();
        let key = match error.kind() {
            ErrorKind::ArgumentConflict => {
                insert_context(error, &mut args, ContextKind::InvalidArg, "argument");
                insert_context(error, &mut args, ContextKind::PriorArg, "other");
                MessageKey::ErrorArgumentConflict
            }
            ErrorKind::InvalidValue => {
                insert_context(error, &mut args, ContextKind::InvalidArg, "argument");
                insert_context(error, &mut args, ContextKind::InvalidValue, "value");
                insert_context(error, &mut args, ContextKind::ValidValue, "valid_values");
                if args.0.get("value").is_none_or(String::is_empty) {
                    MessageKey::ErrorMissingValue
                } else {
                    MessageKey::ErrorInvalidValue
                }
            }
            ErrorKind::UnknownArgument => {
                insert_context(error, &mut args, ContextKind::InvalidArg, "argument");
                MessageKey::ErrorUnknownArgument
            }
            _ => MessageKey::ErrorInvalidCommandLine,
        };
        let empty_args = MessageArgs::default();

        format!(
            "{}: {}\n\n{}: {}\n\n{}\n",
            translator.text(MessageKey::ErrorPrefix, &empty_args),
            translator.text(key, &args),
            translator.text(MessageKey::HelpUsage, &empty_args),
            match translator.language() {
                crate::i18n::Language::ZhCn => "cc-switchy [选项]",
                crate::i18n::Language::Auto | crate::i18n::Language::EnUs => {
                    "cc-switchy [OPTIONS]"
                }
            },
            translator.text(MessageKey::ErrorHelpHint, &empty_args)
        )
    }

    pub fn run_mode(&self) -> RunMode {
        if self.wizard {
            RunMode::Wizard
        } else if self.sync {
            RunMode::Sync {
                source: self.source.clone(),
            }
        } else {
            RunMode::Tui {
                source: self.source.clone(),
            }
        }
    }
}

fn insert_context(
    error: &clap::Error,
    args: &mut MessageArgs,
    context: ContextKind,
    key: &'static str,
) {
    if let Some(value) = error.get(context) {
        args.0.insert(key, value.to_string());
    }
}
