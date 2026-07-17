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
    ProgressWarning,
    ProgressCompleted,
    SyncSummary,
    BackupNotCreated,
    ErrorCancelled,
    ErrorSyncLocked,
    TuiProviders,
    TuiSkills,
    TuiActivity,
    TuiSources,
    TuiAgents,
    TuiDetails,
    TuiResize,
    TuiNoSources,
    TuiNoProviders,
    WizardTitle,
    WizardChooseType,
    WizardConfirmDelete,
    TuiProgress,
    TuiActivityLog,
    TuiStage,
    TuiElapsed,
    TuiReady,
    TuiFailedAgents,
    TuiRetry,
    TuiType,
    TuiEndpoint,
    TuiRemotePath,
    TuiStatus,
    TuiNotTested,
    TuiCurrent,
    TuiAvailable,
    TuiAdditiveSet,
    TuiCategory,
    TuiUnmanaged,
    TuiBytes,
    TuiDefault,
    TuiFooterProviders,
    TuiFooterSkills,
    TuiFooterActivity,
    TuiFooterSources,
    TuiWorking,
    WizardResize,
    WizardNoSources,
    WizardFooterList,
    WizardFooterForm,
    WizardFooterNavigate,
    WizardFooterConfirm,
    WizardFooterBack,
    WizardDetails,
    WizardEditHint,
    WizardSaved,
    WizardTesting,
    WizardConnectedEmpty,
    WizardRequired,
    WizardReplacement,
    WizardLanguage,
    WizardConfirmHint,
    WizardAutoLanguage,
    WizardBackupSettings,
    WizardBackupCreation,
    WizardBackupEnabled,
    WizardBackupDisabled,
    WizardBackupMaxCount,
    WizardBackupUnlimited,
    WizardBackupHint,
    WizardBackupRollbackWarning,
    WizardConfirmDisableBackup,
    WizardFooterBackup,
    WizardBackupInvalidCount,
    FieldName,
    FieldBaseUrl,
    FieldUsername,
    FieldPassword,
    FieldRemoteRoot,
    FieldProfile,
    FieldRegion,
    FieldBucket,
    FieldEndpoint,
    FieldAccessKeyId,
    FieldSecretKey,
    ActivitySyncActive,
    ActivityOperationRunning,
    ActivityCancelRequested,
    ActivityDefaultChanged,
    ActivityWizardBlocked,
    ActivitySwitched,
    ActivityReapplied,
    ActivityRetryComplete,
    ActivitySyncFinished,
    ActivityBackup,
    ActivitySnapshot,
    ActivityConnectedEmpty,
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
        let snapshot = || message_arg(_args, "snapshot");
        let duration = || message_arg(_args, "duration");
        let applied = || message_arg(_args, "applied");
        let skills = || message_arg(_args, "skills");
        let warnings = || message_arg(_args, "warnings");
        let backup = || message_arg(_args, "backup");
        let agent = || message_arg(_args, "agent");
        let provider = || message_arg(_args, "provider");
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
            (Language::ZhCn, MessageKey::ProgressRestoringSkills) => "正在恢复技能",
            (Language::ZhCn, MessageKey::ProgressImportingDatabase) => "正在导入数据库",
            (Language::ZhCn, MessageKey::ProgressApplyingProvider) => "正在应用供应商",
            (Language::ZhCn, MessageKey::ProgressApplyingMcp) => "正在应用 MCP",
            (Language::ZhCn, MessageKey::ProgressApplyingSkills) => "正在应用技能",
            (Language::ZhCn, MessageKey::ProgressRetrying) => "正在重试操作",
            (Language::ZhCn, MessageKey::ProgressWarning) => "警告",
            (Language::ZhCn, MessageKey::ProgressCompleted) => "同步完成",
            (Language::ZhCn, MessageKey::SyncSummary) => {
                return format!(
                    "同步成功\n来源：{}\n快照：{}\n耗时：{}\n恢复 Skills：{}\n应用步骤：{}\n警告：{}\n备份：{}",
                    source(),
                    snapshot(),
                    duration(),
                    skills(),
                    applied(),
                    warnings(),
                    backup()
                );
            }
            (Language::ZhCn, MessageKey::BackupNotCreated) => "备份已关闭；未创建",
            (Language::ZhCn, MessageKey::ErrorCancelled) => "同步已取消",
            (Language::ZhCn, MessageKey::ErrorSyncLocked) => "另一个同步或恢复操作正在运行",
            (Language::ZhCn, MessageKey::TuiProviders) => "供应商",
            (Language::ZhCn, MessageKey::TuiSkills) => "技能",
            (Language::ZhCn, MessageKey::TuiActivity) => "活动",
            (Language::ZhCn, MessageKey::TuiSources) => "同步源",
            (Language::ZhCn, MessageKey::TuiAgents) => "智能体",
            (Language::ZhCn, MessageKey::TuiDetails) => "详情",
            (Language::ZhCn, MessageKey::TuiResize) => "终端过小，请调整到至少 60×18。",
            (Language::ZhCn, MessageKey::TuiNoSources) => {
                "尚未配置同步源。\n请运行：cc-switchy --wizard"
            }
            (Language::ZhCn, MessageKey::TuiNoProviders) => "此智能体没有可用供应商",
            (Language::ZhCn, MessageKey::WizardTitle) => "同步源向导",
            (Language::ZhCn, MessageKey::WizardChooseType) => "选择同步源类型",
            (Language::ZhCn, MessageKey::WizardConfirmDelete) => "确认删除所选同步源？",
            (Language::ZhCn, MessageKey::TuiProgress) => "进度",
            (Language::ZhCn, MessageKey::TuiActivityLog) => "活动记录",
            (Language::ZhCn, MessageKey::TuiStage) => "阶段",
            (Language::ZhCn, MessageKey::TuiElapsed) => "耗时",
            (Language::ZhCn, MessageKey::TuiReady) => "就绪",
            (Language::ZhCn, MessageKey::TuiFailedAgents) => "失败的智能体",
            (Language::ZhCn, MessageKey::TuiRetry) => "重试",
            (Language::ZhCn, MessageKey::TuiType) => "类型",
            (Language::ZhCn, MessageKey::TuiEndpoint) => "端点",
            (Language::ZhCn, MessageKey::TuiRemotePath) => "远程路径",
            (Language::ZhCn, MessageKey::TuiStatus) => "状态",
            (Language::ZhCn, MessageKey::TuiNotTested) => "尚未测试",
            (Language::ZhCn, MessageKey::TuiCurrent) => "当前",
            (Language::ZhCn, MessageKey::TuiAvailable) => "可用",
            (Language::ZhCn, MessageKey::TuiAdditiveSet) => "累加式受管集合",
            (Language::ZhCn, MessageKey::TuiCategory) => "分类",
            (Language::ZhCn, MessageKey::TuiUnmanaged) => "未受管",
            (Language::ZhCn, MessageKey::TuiBytes) => "字节",
            (Language::ZhCn, MessageKey::TuiDefault) => "默认",
            (Language::ZhCn, MessageKey::TuiFooterProviders) => {
                "↑↓ 移动  Tab 切换焦点  [ ] 智能体  Enter 应用  s 同步  w 向导  L 语言  q 退出"
            }
            (Language::ZhCn, MessageKey::TuiFooterSkills) => {
                "↑↓ 移动  Tab 切换焦点  [ ] 智能体  s 同步  w 向导  L 语言  q 退出"
            }
            (Language::ZhCn, MessageKey::TuiFooterActivity) => "s 同步  w 向导  L 语言  q 退出",
            (Language::ZhCn, MessageKey::TuiFooterSources) => {
                "↑↓ 移动  Tab 切换焦点  s 同步  t 测试  m 默认  w 向导  L 语言  q 退出"
            }
            (Language::ZhCn, MessageKey::TuiWorking) => "正在工作 · 恢复前可按 Esc 取消",
            (Language::ZhCn, MessageKey::WizardResize) => "终端过小，请调整到至少 50×15。",
            (Language::ZhCn, MessageKey::WizardNoSources) => {
                "还没有同步源 · 按 a 添加 WebDAV 或 S3"
            }
            (Language::ZhCn, MessageKey::WizardFooterList) => {
                "a 添加  e 编辑  Enter 详情  x 删除  t 测试  m 默认  b 备份  L 语言  q 退出"
            }
            (Language::ZhCn, MessageKey::WizardFooterForm) => {
                "Esc 放弃  Ctrl+C 退出  Tab/Shift+Tab 字段  Enter 下一项/保存  直接输入"
            }
            (Language::ZhCn, MessageKey::WizardFooterNavigate) => {
                "↑↓ 选择  Enter 确认  Esc 返回  q 退出"
            }
            (Language::ZhCn, MessageKey::WizardFooterConfirm) => "Enter 确认  Esc 取消  q 退出",
            (Language::ZhCn, MessageKey::WizardFooterBack) => "Enter/Esc 返回  q 退出",
            (Language::ZhCn, MessageKey::WizardDetails) => "详情",
            (Language::ZhCn, MessageKey::WizardEditHint) => {
                "编辑 · Tab 切换字段 · Enter 下一项/保存 · Esc 放弃"
            }
            (Language::ZhCn, MessageKey::WizardSaved) => "✓ 已保存",
            (Language::ZhCn, MessageKey::WizardTesting) => "正在测试…",
            (Language::ZhCn, MessageKey::WizardConnectedEmpty) => "! 已连接，但没有可用快照",
            (Language::ZhCn, MessageKey::WizardRequired) => "请填写所有必填字段",
            (Language::ZhCn, MessageKey::WizardReplacement) => "选择新的默认同步源",
            (Language::ZhCn, MessageKey::WizardLanguage) => "语言",
            (Language::ZhCn, MessageKey::WizardConfirmHint) => "Enter 确认 · Esc 取消",
            (Language::ZhCn, MessageKey::WizardAutoLanguage) => "跟随系统",
            (Language::ZhCn, MessageKey::WizardBackupSettings) => "备份设置",
            (Language::ZhCn, MessageKey::WizardBackupCreation) => "创建备份",
            (Language::ZhCn, MessageKey::WizardBackupEnabled) => "开启",
            (Language::ZhCn, MessageKey::WizardBackupDisabled) => "关闭",
            (Language::ZhCn, MessageKey::WizardBackupMaxCount) => "最多保留",
            (Language::ZhCn, MessageKey::WizardBackupUnlimited) => "不限数量",
            (Language::ZhCn, MessageKey::WizardBackupHint) => {
                "0 表示不限数量；正数会在下次开启备份的同步时只保留最新数量。"
            }
            (Language::ZhCn, MessageKey::WizardBackupRollbackWarning) => {
                "关闭后不会创建备份，恢复失败时也无法回滚。"
            }
            (Language::ZhCn, MessageKey::WizardConfirmDisableBackup) => {
                "关闭备份后，本次及后续同步不会创建本地备份；恢复失败时无法回滚。确定关闭？"
            }
            (Language::ZhCn, MessageKey::WizardFooterBackup) => {
                "↑↓/Tab 字段  空格 开关  数字 数量  Enter 保存  Esc 返回  Ctrl+C 退出"
            }
            (Language::ZhCn, MessageKey::WizardBackupInvalidCount) => {
                "请输入 0 或更大的整数"
            }
            (Language::ZhCn, MessageKey::FieldName) => "名称",
            (Language::ZhCn, MessageKey::FieldBaseUrl) => "基础 URL",
            (Language::ZhCn, MessageKey::FieldUsername) => "用户名",
            (Language::ZhCn, MessageKey::FieldPassword) => "密码",
            (Language::ZhCn, MessageKey::FieldRemoteRoot) => "远程根目录",
            (Language::ZhCn, MessageKey::FieldProfile) => "配置档",
            (Language::ZhCn, MessageKey::FieldRegion) => "区域",
            (Language::ZhCn, MessageKey::FieldBucket) => "存储桶",
            (Language::ZhCn, MessageKey::FieldEndpoint) => "端点",
            (Language::ZhCn, MessageKey::FieldAccessKeyId) => "访问密钥 ID",
            (Language::ZhCn, MessageKey::FieldSecretKey) => "秘密访问密钥",
            (Language::ZhCn, MessageKey::ActivitySyncActive) => "同步正在运行，请先取消再退出。",
            (Language::ZhCn, MessageKey::ActivityOperationRunning) => "另一个操作正在运行。",
            (Language::ZhCn, MessageKey::ActivityCancelRequested) => "已请求取消。",
            (Language::ZhCn, MessageKey::ActivityDefaultChanged) => {
                return format!("默认同步源已切换为 {}。", source());
            }
            (Language::ZhCn, MessageKey::ActivityWizardBlocked) => {
                "请先完成或取消当前同步，再打开向导。"
            }
            (Language::ZhCn, MessageKey::ActivitySwitched) => {
                return format!("{} 已切换到 {}。", agent(), provider());
            }
            (Language::ZhCn, MessageKey::ActivityReapplied) => {
                return format!("已重新应用 {} 的供应商。", agent());
            }
            (Language::ZhCn, MessageKey::ActivityRetryComplete) => "投影重试完成。",
            (Language::ZhCn, MessageKey::ActivitySyncFinished) => {
                return format!("同步完成，警告 {} 项。", warnings());
            }
            (Language::ZhCn, MessageKey::ActivityBackup) => {
                return format!("备份：{}", backup());
            }
            (Language::ZhCn, MessageKey::ActivitySnapshot) => {
                return format!("✓ 快照 {}", snapshot());
            }
            (Language::ZhCn, MessageKey::ActivityConnectedEmpty) => "! 已连接，但没有快照",
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
            (Language::Auto | Language::EnUs, MessageKey::ProgressWarning) => "Warning",
            (Language::Auto | Language::EnUs, MessageKey::ProgressCompleted) => "Sync completed",
            (Language::Auto | Language::EnUs, MessageKey::SyncSummary) => {
                return format!(
                    "Sync succeeded\nSource: {}\nSnapshot: {}\nDuration: {}\nRestored Skills: {}\nApplied steps: {}\nWarnings: {}\nBackup: {}",
                    source(),
                    snapshot(),
                    duration(),
                    skills(),
                    applied(),
                    warnings(),
                    backup()
                );
            }
            (Language::Auto | Language::EnUs, MessageKey::BackupNotCreated) => {
                "Backup disabled; not created"
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorCancelled) => {
                "Synchronization was cancelled"
            }
            (Language::Auto | Language::EnUs, MessageKey::ErrorSyncLocked) => {
                "Another sync or restore operation is already running"
            }
            (Language::Auto | Language::EnUs, MessageKey::TuiProviders) => "Providers",
            (Language::Auto | Language::EnUs, MessageKey::TuiSkills) => "Skills",
            (Language::Auto | Language::EnUs, MessageKey::TuiActivity) => "Activity",
            (Language::Auto | Language::EnUs, MessageKey::TuiSources) => "Sources",
            (Language::Auto | Language::EnUs, MessageKey::TuiAgents) => "Agents",
            (Language::Auto | Language::EnUs, MessageKey::TuiDetails) => "Details",
            (Language::Auto | Language::EnUs, MessageKey::TuiResize) => {
                "Terminal is too small. Resize to at least 60×18."
            }
            (Language::Auto | Language::EnUs, MessageKey::TuiNoSources) => {
                "No sync source is configured.\nRun: cc-switchy --wizard"
            }
            (Language::Auto | Language::EnUs, MessageKey::TuiNoProviders) => {
                "No providers are available for this Agent"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardTitle) => "Sync Sources Wizard",
            (Language::Auto | Language::EnUs, MessageKey::WizardChooseType) => {
                "Choose a source type"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardConfirmDelete) => {
                "Delete the selected source?"
            }
            (Language::Auto | Language::EnUs, MessageKey::TuiProgress) => "Progress",
            (Language::Auto | Language::EnUs, MessageKey::TuiActivityLog) => "Activity log",
            (Language::Auto | Language::EnUs, MessageKey::TuiStage) => "Stage",
            (Language::Auto | Language::EnUs, MessageKey::TuiElapsed) => "Elapsed",
            (Language::Auto | Language::EnUs, MessageKey::TuiReady) => "Ready",
            (Language::Auto | Language::EnUs, MessageKey::TuiFailedAgents) => "Failed Agents",
            (Language::Auto | Language::EnUs, MessageKey::TuiRetry) => "retry",
            (Language::Auto | Language::EnUs, MessageKey::TuiType) => "Type",
            (Language::Auto | Language::EnUs, MessageKey::TuiEndpoint) => "Endpoint",
            (Language::Auto | Language::EnUs, MessageKey::TuiRemotePath) => "Remote path",
            (Language::Auto | Language::EnUs, MessageKey::TuiStatus) => "Status",
            (Language::Auto | Language::EnUs, MessageKey::TuiNotTested) => "Not tested",
            (Language::Auto | Language::EnUs, MessageKey::TuiCurrent) => "current",
            (Language::Auto | Language::EnUs, MessageKey::TuiAvailable) => "available",
            (Language::Auto | Language::EnUs, MessageKey::TuiAdditiveSet) => "additive managed set",
            (Language::Auto | Language::EnUs, MessageKey::TuiCategory) => "Category",
            (Language::Auto | Language::EnUs, MessageKey::TuiUnmanaged) => "unmanaged",
            (Language::Auto | Language::EnUs, MessageKey::TuiBytes) => "bytes",
            (Language::Auto | Language::EnUs, MessageKey::TuiDefault) => "DEFAULT",
            (Language::Auto | Language::EnUs, MessageKey::TuiFooterProviders) => {
                "↑↓ move  Tab focus  [ ] Agent  Enter apply  s sync  w wizard  L language  q quit"
            }
            (Language::Auto | Language::EnUs, MessageKey::TuiFooterSkills) => {
                "↑↓ move  Tab focus  [ ] Agent  s sync  w wizard  L language  q quit"
            }
            (Language::Auto | Language::EnUs, MessageKey::TuiFooterActivity) => {
                "s sync  w wizard  L language  q quit"
            }
            (Language::Auto | Language::EnUs, MessageKey::TuiFooterSources) => {
                "↑↓ move  Tab focus  s sync  t test  m default  w wizard  L language  q quit"
            }
            (Language::Auto | Language::EnUs, MessageKey::TuiWorking) => {
                "working · Esc cancels before restore"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardResize) => {
                "Resize terminal to at least 50×15"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardNoSources) => {
                "No sources yet · press a to add WebDAV or S3"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardFooterList) => {
                "a add  e edit  Enter details  x delete  t test  m default  b backup  L language  q exit"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardFooterForm) => {
                "Esc discard  Ctrl+C exit  Tab/Shift+Tab field  Enter next/save  type to edit"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardFooterNavigate) => {
                "↑↓ choose  Enter confirm  Esc back  q exit"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardFooterConfirm) => {
                "Enter confirm  Esc cancel  q exit"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardFooterBack) => {
                "Enter/Esc back  q exit"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardDetails) => "Details",
            (Language::Auto | Language::EnUs, MessageKey::WizardEditHint) => {
                "Edit · Tab field · Enter next/save · Esc discard"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardSaved) => "✓ Saved",
            (Language::Auto | Language::EnUs, MessageKey::WizardTesting) => "Testing…",
            (Language::Auto | Language::EnUs, MessageKey::WizardConnectedEmpty) => {
                "! Connected, no snapshot available"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardRequired) => {
                "All required fields must be filled"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardReplacement) => {
                "Choose replacement default"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardLanguage) => "Language",
            (Language::Auto | Language::EnUs, MessageKey::WizardConfirmHint) => {
                "Enter confirm · Esc cancel"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardAutoLanguage) => "Auto",
            (Language::Auto | Language::EnUs, MessageKey::WizardBackupSettings) => {
                "Backup Settings"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardBackupCreation) => {
                "Create backups"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardBackupEnabled) => "Enabled",
            (Language::Auto | Language::EnUs, MessageKey::WizardBackupDisabled) => "Disabled",
            (Language::Auto | Language::EnUs, MessageKey::WizardBackupMaxCount) => "Maximum kept",
            (Language::Auto | Language::EnUs, MessageKey::WizardBackupUnlimited) => "Unlimited",
            (Language::Auto | Language::EnUs, MessageKey::WizardBackupHint) => {
                "0 keeps unlimited backups; a positive value keeps the newest count on the next enabled sync."
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardBackupRollbackWarning) => {
                "Disabling backups also makes rollback unavailable after a restore failure."
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardConfirmDisableBackup) => {
                "Disabling backups creates no local backup for this or future syncs, so restore failures cannot roll back. Disable backups?"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardFooterBackup) => {
                "↑↓/Tab field  Space toggle  digits count  Enter save  Esc back  Ctrl+C exit"
            }
            (Language::Auto | Language::EnUs, MessageKey::WizardBackupInvalidCount) => {
                "Enter 0 or a larger whole number"
            }
            (Language::Auto | Language::EnUs, MessageKey::FieldName) => "Name",
            (Language::Auto | Language::EnUs, MessageKey::FieldBaseUrl) => "Base URL",
            (Language::Auto | Language::EnUs, MessageKey::FieldUsername) => "Username",
            (Language::Auto | Language::EnUs, MessageKey::FieldPassword) => "Password",
            (Language::Auto | Language::EnUs, MessageKey::FieldRemoteRoot) => "Remote root",
            (Language::Auto | Language::EnUs, MessageKey::FieldProfile) => "Profile",
            (Language::Auto | Language::EnUs, MessageKey::FieldRegion) => "Region",
            (Language::Auto | Language::EnUs, MessageKey::FieldBucket) => "Bucket",
            (Language::Auto | Language::EnUs, MessageKey::FieldEndpoint) => "Endpoint",
            (Language::Auto | Language::EnUs, MessageKey::FieldAccessKeyId) => "Access key ID",
            (Language::Auto | Language::EnUs, MessageKey::FieldSecretKey) => "Secret key",
            (Language::Auto | Language::EnUs, MessageKey::ActivitySyncActive) => {
                "A sync is active. Cancel it before quitting."
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivityOperationRunning) => {
                "Another operation is already running."
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivityCancelRequested) => {
                "Cancellation requested."
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivityDefaultChanged) => {
                return format!("Default source changed to {}.", source());
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivityWizardBlocked) => {
                "Finish or cancel the active sync before opening the wizard."
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivitySwitched) => {
                return format!("{} switched to {}.", agent(), provider());
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivityReapplied) => {
                return format!("{} providers reapplied.", agent());
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivityRetryComplete) => {
                "Projection retry completed."
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivitySyncFinished) => {
                return format!("Sync finished with {} warning(s).", warnings());
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivityBackup) => {
                return format!("Backup: {}", backup());
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivitySnapshot) => {
                return format!("✓ Snapshot {}", snapshot());
            }
            (Language::Auto | Language::EnUs, MessageKey::ActivityConnectedEmpty) => {
                "! Connected, no snapshot"
            }
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
