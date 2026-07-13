use std::collections::BTreeMap;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Language {
    #[serde(rename = "auto")]
    #[default]
    Auto,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en-US")]
    EnUs,
}

impl Language {
    pub fn resolve(cli_override: Option<&str>, persisted: Option<Self>) -> Self {
        cli_override
            .and_then(Self::from_tag)
            .or_else(|| {
                std::env::var("CC_SWITCHY_LANG")
                    .ok()
                    .as_deref()
                    .and_then(Self::from_tag)
            })
            .or_else(|| persisted.filter(|language| *language != Self::Auto))
            .or_else(Self::from_locale)
            .unwrap_or(Self::EnUs)
    }

    fn from_locale() -> Option<Self> {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok())
            .as_deref()
            .and_then(Self::from_tag)
    }

    fn from_tag(value: &str) -> Option<Self> {
        let tag = value.trim().to_ascii_lowercase();
        if tag == "zh" || tag.starts_with("zh_") || tag.starts_with("zh-") {
            Some(Self::ZhCn)
        } else if tag == "en" || tag.starts_with("en_") || tag.starts_with("en-") {
            Some(Self::EnUs)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageKey {
    NoSourceConfigured,
    RunWizard,
    UnexpectedError,
    HomeDirectoryUnavailable,
    HelpAbout,
    HelpUsage,
    HelpOptions,
    HelpWizard,
    HelpSync,
    HelpSource,
    HelpLanguage,
    HelpFlag,
    VersionFlag,
    ModeTui,
    ModeWizard,
    ModeSync,
    ErrorPrefix,
    ErrorArgumentConflict,
    ErrorInvalidValue,
    ErrorMissingValue,
    ErrorUnknownArgument,
    ErrorInvalidCommandLine,
    ErrorHelpHint,
}

#[derive(Debug, Default)]
pub struct MessageArgs(pub BTreeMap<&'static str, String>);

pub struct Translator {
    language: Language,
}

impl Translator {
    pub fn new(language: Language) -> Self {
        let language = match language {
            Language::Auto => Language::resolve(None, None),
            language => language,
        };
        Self { language }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn text(&self, key: MessageKey, _args: &MessageArgs) -> String {
        let argument = || message_arg(_args, "argument");
        let other = || message_arg(_args, "other");
        let value = || message_arg(_args, "value");
        let valid_values = || message_arg(_args, "valid_values");
        let text = match (self.language, key) {
            (Language::ZhCn, MessageKey::NoSourceConfigured) => "尚未配置来源。",
            (Language::ZhCn, MessageKey::RunWizard) => "请运行：cc-switchy --wizard",
            (Language::ZhCn, MessageKey::UnexpectedError) => "发生意外错误",
            (Language::ZhCn, MessageKey::HomeDirectoryUnavailable) => "无法确定用户主目录",
            (Language::ZhCn, MessageKey::HelpAbout) => "从云端恢复 CC Switch 配置到本机",
            (Language::ZhCn, MessageKey::HelpUsage) => "用法",
            (Language::ZhCn, MessageKey::HelpOptions) => "选项",
            (Language::ZhCn, MessageKey::HelpWizard) => "运行配置向导",
            (Language::ZhCn, MessageKey::HelpSync) => "以非交互方式同步配置",
            (Language::ZhCn, MessageKey::HelpSource) => "选择要使用的来源",
            (Language::ZhCn, MessageKey::HelpLanguage) => "选择界面语言（zh 或 en）",
            (Language::ZhCn, MessageKey::HelpFlag) => "显示帮助",
            (Language::ZhCn, MessageKey::VersionFlag) => "显示版本",
            (Language::ZhCn, MessageKey::ModeTui) => "交互界面",
            (Language::ZhCn, MessageKey::ModeWizard) => "配置向导",
            (Language::ZhCn, MessageKey::ModeSync) => "同步",
            (Language::ZhCn, MessageKey::ErrorPrefix) => "错误",
            (Language::ZhCn, MessageKey::ErrorArgumentConflict) => {
                return format!("参数“{}”不能与“{}”同时使用", argument(), other());
            }
            (Language::ZhCn, MessageKey::ErrorInvalidValue) => {
                let possible_values = if valid_values().is_empty() {
                    String::new()
                } else {
                    format!("（可选值：{}）", valid_values())
                };
                return format!("参数“{}”的值“{}”无效{possible_values}", argument(), value());
            }
            (Language::ZhCn, MessageKey::ErrorMissingValue) => {
                return format!("参数“{}”需要一个值", argument());
            }
            (Language::ZhCn, MessageKey::ErrorUnknownArgument) => {
                return format!("发现未知参数“{}”", argument());
            }
            (Language::ZhCn, MessageKey::ErrorInvalidCommandLine) => "命令行参数无效",
            (Language::ZhCn, MessageKey::ErrorHelpHint) => "更多信息请运行“--help”。",
            (Language::Auto | Language::EnUs, MessageKey::NoSourceConfigured) => {
                "No source is configured."
            }
            (Language::Auto | Language::EnUs, MessageKey::RunWizard) => "Run: cc-switchy --wizard",
            (Language::Auto | Language::EnUs, MessageKey::UnexpectedError) => "Unexpected error",
            (Language::Auto | Language::EnUs, MessageKey::HomeDirectoryUnavailable) => {
                "Home directory is unavailable"
            }
            (Language::Auto | Language::EnUs, MessageKey::HelpAbout) => {
                "Restore CC Switch cloud configuration to this machine"
            }
            (Language::Auto | Language::EnUs, MessageKey::HelpUsage) => "Usage",
            (Language::Auto | Language::EnUs, MessageKey::HelpOptions) => "Options",
            (Language::Auto | Language::EnUs, MessageKey::HelpWizard) => {
                "Run the configuration wizard"
            }
            (Language::Auto | Language::EnUs, MessageKey::HelpSync) => {
                "Synchronize configuration non-interactively"
            }
            (Language::Auto | Language::EnUs, MessageKey::HelpSource) => "Select the source to use",
            (Language::Auto | Language::EnUs, MessageKey::HelpLanguage) => {
                "Select the interface language (zh or en)"
            }
            (Language::Auto | Language::EnUs, MessageKey::HelpFlag) => "Print help",
            (Language::Auto | Language::EnUs, MessageKey::VersionFlag) => "Print version",
            (Language::Auto | Language::EnUs, MessageKey::ModeTui) => "interactive interface",
            (Language::Auto | Language::EnUs, MessageKey::ModeWizard) => "configuration wizard",
            (Language::Auto | Language::EnUs, MessageKey::ModeSync) => "sync",
            (Language::Auto | Language::EnUs, MessageKey::ErrorPrefix) => "error",
            (Language::Auto | Language::EnUs, MessageKey::ErrorArgumentConflict) => {
                return format!(
                    "the argument '{}' cannot be used with '{}'",
                    argument(),
                    other()
                );
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorInvalidValue) => {
                let possible_values = if valid_values().is_empty() {
                    String::new()
                } else {
                    format!(" (possible values: {})", valid_values())
                };
                return format!(
                    "invalid value '{}' for '{}'{possible_values}",
                    value(),
                    argument()
                );
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorMissingValue) => {
                return format!("a value is required for '{}'", argument());
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorUnknownArgument) => {
                return format!("unexpected argument '{}' found", argument());
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorInvalidCommandLine) => {
                "invalid command-line arguments"
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorHelpHint) => {
                "For more information, try '--help'."
            }
        };
        text.to_owned()
    }
}

fn message_arg<'a>(args: &'a MessageArgs, key: &'static str) -> &'a str {
    args.0.get(key).map(String::as_str).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::Language;

    #[test]
    fn language_serialization_matches_the_config_contract() {
        assert_eq!(serde_json::to_string(&Language::Auto).unwrap(), "\"auto\"");
        assert_eq!(serde_json::to_string(&Language::ZhCn).unwrap(), "\"zh-CN\"");
        assert_eq!(serde_json::to_string(&Language::EnUs).unwrap(), "\"en-US\"");
    }
}
