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
    ProgressLocking,
    ProgressConnecting,
    ProgressFetchingManifest,
    ProgressValidatingManifest,
    ProgressDownloading,
    ProgressVerifying,
    ProgressPreparingBackup,
    ProgressRestoringSkills,
    ProgressImportingDatabase,
    ProgressApplyingProvider,
    ProgressApplyingMcp,
    ProgressApplyingSkills,
    ProgressRetrying,
    ProgressCompleted,
    ErrorManifestTooLarge,
    ErrorManifestParse,
    ErrorManifestFormat,
    ErrorManifestVersion,
    ErrorDatabaseVersionMissing,
    ErrorDatabaseVersion,
    ErrorMissingArtifact,
    ErrorArtifactTooLarge,
    ErrorInvalidArtifactHash,
    ErrorSnapshotIdMismatch,
    ErrorArtifactSizeMismatch,
    ErrorArtifactHashMismatch,
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
        let source = || message_arg(_args, "source");
        let artifact = || message_arg(_args, "artifact");
        let found = || message_arg(_args, "found");
        let supported = || message_arg(_args, "supported");
        let size = || message_arg(_args, "size");
        let max = || message_arg(_args, "max");
        let expected = || message_arg(_args, "expected");
        let actual = || message_arg(_args, "actual");
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
            (Language::ZhCn, MessageKey::ProgressLocking) => "正在获取同步锁",
            (Language::ZhCn, MessageKey::ProgressConnecting) => {
                return format!("正在连接同步源 {}", source());
            }
            (Language::ZhCn, MessageKey::ProgressFetchingManifest) => "正在读取 manifest",
            (Language::ZhCn, MessageKey::ProgressValidatingManifest) => "正在校验 manifest",
            (Language::ZhCn, MessageKey::ProgressDownloading) => {
                return format!("正在下载 {}", artifact());
            }
            (Language::ZhCn, MessageKey::ProgressVerifying) => {
                return format!("正在校验 {}", artifact());
            }
            (Language::ZhCn, MessageKey::ProgressPreparingBackup) => "正在备份本地数据",
            (Language::ZhCn, MessageKey::ProgressRestoringSkills) => "正在恢复 Skills",
            (Language::ZhCn, MessageKey::ProgressImportingDatabase) => "正在导入数据库",
            (Language::ZhCn, MessageKey::ProgressApplyingProvider) => "正在应用供应商",
            (Language::ZhCn, MessageKey::ProgressApplyingMcp) => "正在应用 MCP",
            (Language::ZhCn, MessageKey::ProgressApplyingSkills) => "正在应用 Skills",
            (Language::ZhCn, MessageKey::ProgressRetrying) => "正在重试操作",
            (Language::ZhCn, MessageKey::ProgressCompleted) => "同步完成",
            (Language::ZhCn, MessageKey::ErrorManifestTooLarge) => {
                return format!("manifest 大小为 {} 字节，超过 {} 字节上限", size(), max());
            }
            (Language::ZhCn, MessageKey::ErrorManifestParse) => "manifest JSON 无效",
            (Language::ZhCn, MessageKey::ErrorManifestFormat) => {
                return format!("manifest 格式不兼容：{}", found());
            }
            (Language::ZhCn, MessageKey::ErrorManifestVersion) => {
                return format!(
                    "manifest 协议版本 {} 与本地版本 {} 不兼容",
                    found(),
                    supported()
                );
            }
            (Language::ZhCn, MessageKey::ErrorDatabaseVersionMissing) => {
                "manifest 缺少数据库兼容版本"
            }
            (Language::ZhCn, MessageKey::ErrorDatabaseVersion) => {
                return format!(
                    "数据库兼容版本 {} 与本地版本 {} 不兼容",
                    found(),
                    supported()
                );
            }
            (Language::ZhCn, MessageKey::ErrorMissingArtifact) => {
                return format!("manifest 缺少必需文件 {}", artifact());
            }
            (Language::ZhCn, MessageKey::ErrorArtifactTooLarge) => {
                return format!(
                    "文件 {} 大小为 {} 字节，超过 {} 字节上限",
                    artifact(),
                    size(),
                    max()
                );
            }
            (Language::ZhCn, MessageKey::ErrorInvalidArtifactHash) => {
                return format!("文件 {} 的 SHA-256 无效", artifact());
            }
            (Language::ZhCn, MessageKey::ErrorSnapshotIdMismatch) => "snapshotId 与文件哈希不匹配",
            (Language::ZhCn, MessageKey::ErrorArtifactSizeMismatch) => {
                return format!(
                    "文件 {} 大小不匹配：应为 {}，实际为 {}",
                    artifact(),
                    expected(),
                    actual()
                );
            }
            (Language::ZhCn, MessageKey::ErrorArtifactHashMismatch) => {
                return format!("文件 {} 的 SHA-256 校验失败", artifact());
            }
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
            (Language::Auto | Language::EnUs, MessageKey::ProgressLocking) => {
                "Acquiring the sync lock"
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressConnecting) => {
                return format!("Connecting to source {}", source());
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressFetchingManifest) => {
                "Fetching the manifest"
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressValidatingManifest) => {
                "Validating the manifest"
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressDownloading) => {
                return format!("Downloading {}", artifact());
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressVerifying) => {
                return format!("Verifying {}", artifact());
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressPreparingBackup) => {
                "Backing up local data"
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressRestoringSkills) => {
                "Restoring Skills"
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressImportingDatabase) => {
                "Importing the database"
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressApplyingProvider) => {
                "Applying providers"
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressApplyingMcp) => "Applying MCP",
            (Language::Auto | Language::EnUs, MessageKey::ProgressApplyingSkills) => {
                "Applying Skills"
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressRetrying) => {
                "Retrying the operation"
            }
            (Language::Auto | Language::EnUs, MessageKey::ProgressCompleted) => "Sync completed",
            (Language::Auto | Language::EnUs, MessageKey::ErrorManifestTooLarge) => {
                return format!("manifest size {} exceeds the {} byte limit", size(), max());
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorManifestParse) => {
                "manifest JSON is invalid"
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorManifestFormat) => {
                return format!("manifest format is incompatible: {}", found());
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorManifestVersion) => {
                return format!(
                    "manifest protocol version {} is incompatible with local version {}",
                    found(),
                    supported()
                );
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorDatabaseVersionMissing) => {
                "manifest is missing the database compatibility version"
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorDatabaseVersion) => {
                return format!(
                    "database compatibility version {} is incompatible with local version {}",
                    found(),
                    supported()
                );
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorMissingArtifact) => {
                return format!("manifest is missing required artifact {}", artifact());
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorArtifactTooLarge) => {
                return format!(
                    "artifact {} size {} exceeds the {} byte limit",
                    artifact(),
                    size(),
                    max()
                );
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorInvalidArtifactHash) => {
                return format!("artifact {} has an invalid SHA-256 value", artifact());
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorSnapshotIdMismatch) => {
                "snapshotId does not match the artifact hashes"
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorArtifactSizeMismatch) => {
                return format!(
                    "artifact {} size mismatch: expected {}, found {}",
                    artifact(),
                    expected(),
                    actual()
                );
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorArtifactHashMismatch) => {
                return format!("artifact {} failed SHA-256 verification", artifact());
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
