# cc-switchy

[简体中文](#简体中文) · [English](#english)

`cc-switchy` is a download-only Rust CLI and Ratatui application for restoring
CC Switch cloud snapshots from WebDAV or S3-compatible storage. It can manage
multiple sources, restore the latest selected snapshot, and apply providers,
MCP servers, and Skills to supported local Agents.

> `cc-switchy --sync` always downloads and applies the current snapshot from
> the selected source. It does not upload, merge, or delete remote data.

## 简体中文

### 功能

- 默认启动键盘优先的 Ratatui TUI。
- 通过向导管理多个 WebDAV 与 S3 同步源：列表、新增、查看、编辑、删除、测试和设置默认源。
- `cc-switchy --sync` 一键重新下载并恢复默认源的最新快照。
- 安全恢复 CC Switch v2/db-v6 的 `manifest.json`、`db.sql` 和 `skills.zip`，兼容 WebDAV db-v5 旧路径回退。
- 将供应商、MCP 和 Skills 应用到本地 Agent，并在 TUI 中切换独占式 Agent 的供应商。
- CLI、向导、TUI、进度与错误均支持简体中文和英文。
- 全程显示阶段、下载字节、耗时、警告和备份位置。

### 安装

从 GitHub Releases 下载与系统对应的压缩包，校验 `SHA256SUMS` 后，将
`cc-switchy`（Windows 为 `cc-switchy.exe`）放入 `PATH`。

也可以从源码构建：

```bash
git clone https://github.com/ca-x/cc-switchy.git
cd cc-switchy
cargo build --release --locked
install -Dm755 target/release/cc-switchy ~/.local/bin/cc-switchy
```

发布包含以下目标：

- `x86_64-unknown-linux-gnu`
- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

Linux GNU 构建依赖构建环境的 glibc 基线。需要更强的发行版兼容性、静态
运行时或容器部署时，优先使用 MUSL 构建。

### 首次运行与命令

```bash
cc-switchy
cc-switchy --wizard
cc-switchy --sync
cc-switchy --sync --source backup-s3
cc-switchy --lang zh
cc-switchy --lang en
```

- `cc-switchy`：打开 TUI。没有同步源时显示 `cc-switchy --wizard` 引导。
- `cc-switchy --wizard`：打开同步源管理向导。
- `cc-switchy --sync`：从默认源重新下载、校验、恢复并应用当前快照。
- `cc-switchy --sync --source backup-s3`：只在本次运行使用 `backup-s3`，不修改默认源。
- `--lang zh|en`：只覆盖本次运行的语言；向导/TUI 中的语言操作会持久化偏好。

### 向导按键

- `a` 新增 WebDAV 或 S3 来源
- 表单内直接输入文字，`Tab/Shift+Tab` 切换字段
- `Enter` 查看详情、进入下一字段或保存
- `e` 编辑，`x` 删除，`t` 测试连接，`m` 设置默认源
- `L` 切换语言
- `Esc` 放弃当前表单或返回上一层
- `q` 在非表单界面退出，`Ctrl+C` 可从任意向导界面退出

首次新增的来源自动成为默认源。删除默认源时，如果仍有其他来源，必须先
选择替代默认源。密码和 Secret Access Key 完全遮挡，Access Key ID 只显示
有限前缀。

### TUI 按键

- `1` 供应商，`2` Skills，`3` 活动，`4` 同步源
- `↑/↓` 或 `j/k` 移动，`Tab/Shift+Tab` 切换焦点，`←/→` 或 `h/l` 切换面板
- `[`/`]` 切换 Agent；此操作只浏览，不会切换供应商
- `Enter` 切换独占式 Agent 的供应商，或重新应用累加式 Agent 的受管供应商集合
- `s` 同步默认源；在“同步源”页同步当前选中源
- `t` 测试选中源，`m` 设置默认源，`w` 打开向导，`L` 切换语言
- `Esc` 在本地恢复开始前请求取消，`q` 安全退出

导航没有装饰动画。只有真实同步或应用操作才刷新进度，终端 raw mode、备用
屏幕和光标状态会在正常退出及 panic 路径恢复。

### 配置文件

`cc-switchy` 自有配置位于 `~/.cc-switchy`：

```text
~/.cc-switchy/
├── config.toml
├── config.toml.bak
├── state.json
├── lock
├── staging/
└── backups/
```

CC Switch 兼容数据库、设备设置和默认 Skills SSOT 保留在 `~/.cc-switch`。
如果本机 CC Switch 设置使用 `~/.agents/skills`，则按该 SSOT 应用 Skills。

示例配置：

```toml
version = 1
language = "auto"
default_source = "home-webdav"

[[sources]]
name = "home-webdav"
type = "webdav"
remote_root = "cc-switch-sync"
profile = "default"

[sources.webdav]
base_url = "https://dav.example.com/remote.php/dav/files/user"
username = "user"
password = "replace-me"

[[sources]]
name = "backup-s3"
type = "s3"
remote_root = "cc-switch-sync"
profile = "default"

[sources.s3]
region = "auto"
bucket = "cc-switch"
endpoint = "https://account.r2.cloudflarestorage.com"
access_key_id = "replace-me"
secret_access_key = "replace-me"
```

空 S3 `endpoint` 使用 AWS 虚拟主机风格；自定义端点使用 path-style。未写协议
的自定义端点默认使用 HTTPS。

### 同步、恢复与回滚

显式同步每次都重新获取 manifest 和两个制品；云端所选快照是本次恢复的权威
来源，即使本地数据更新也不会做双向合并。流程会先校验协议、大小、SHA-256、
ZIP 路径和 SQL，再创建本地备份并替换数据库与 Skills。

备份保存在 `~/.cc-switchy/backups/<timestamp>/`。数据库替换失败时会回滚 Skills；
任何 Agent 投影失败会成为警告，其他 Agent 继续应用。

- Exit code `0`：恢复及所有本地投影成功。
- Exit code `1`：配置、网络、校验、恢复或回滚失败。
- Exit code `2`：数据库和 Skills 已恢复，但一个或多个 Agent 投影产生警告。

### Agent 能力

| Agent | 供应商模式 | MCP | Skills |
| --- | --- | --- | --- |
| Claude | 独占 | 支持 | 支持 |
| Claude Desktop | 独占；仅 macOS/Windows，代理模式为警告 | 不支持 | 不支持 |
| Codex | 独占 | 支持 | 支持 |
| Gemini | 独占 | 支持 | 支持 |
| OpenCode | 累加 | 支持 | 支持 |
| OpenClaw | 累加 | 不支持 | 不支持 |
| Hermes | 累加 | 支持 | 支持 |

### 安全说明

> CC Switch 快照协议不加密内容。`db.sql` 可能包含供应商 API Key、访问令牌和
> 其他明文秘密。必须保护 WebDAV/S3 存储、账户、网络访问和本地备份。

- `cc-switchy` 只实现 GET/HEAD 与只读连接诊断，不包含远程上传、删除或建桶接口。
- `config.toml` 与 `state.json` 在 Unix 上使用 `0600`；凭据仍以兼容所需的明文保存。
- 下载大小、ZIP 展开路径/数量/总量、SQL 导入和 SHA-256 均在修改本地状态前校验。
- Reqwest 使用 Rustls 与 webpki roots。依赖仅由私有 CA 签发的 WebDAV/S3 端点
  默认不会被信任；当前版本不提供自定义 CA 导入参数。
- 日志和 TUI 会遮挡密码、Authorization、S3 签名和 URL 查询值，但操作系统、
  终端录制和备份权限仍需由使用者保护。

## English

### What it does

- Opens a keyboard-first Ratatui TUI by default.
- Manages multiple WebDAV and S3 sources with CRUD, connection tests, and one default source.
- Re-fetches and restores the current selected CC Switch snapshot with one `--sync` command.
- Safely validates and restores CC Switch v2/db-v6 `manifest.json`, `db.sql`, and `skills.zip` artifacts.
- Projects providers, MCP servers, and Skills to supported local Agents and switches exclusive providers from the TUI.
- Renders CLI, Wizard, TUI, progress, and errors in English or Simplified Chinese.

### Quick start

Download a target archive from GitHub Releases, verify it against
`SHA256SUMS`, and place the executable on `PATH`. The six target triples are
listed in the Chinese installation section above.

```bash
cc-switchy --wizard
cc-switchy
cc-switchy --sync
cc-switchy --sync --source backup-s3
cc-switchy --lang en
cc-switchy --lang zh
```

The first source becomes the default. `--source` overrides it only for the
current invocation. Every explicit sync re-downloads the current remote
snapshot and overwrites/repairs compatible local state; there is no timestamp
comparison, merge, upload, remote delete, or conflict resolution.

### Wizard and TUI

Wizard keys: printable characters enter text inside forms, `Tab/Shift+Tab`
changes fields, `Enter` inspects/advances/saves, `a` adds, `e` edits, `x`
deletes, `t` tests, `m` makes the selected source default, and `L` changes
language. `Esc` discards a form or goes back, `q` exits outside forms, and
`Ctrl+C` exits from every Wizard screen.

Main TUI keys: `1` Providers, `2` Skills, `3` Activity, `4` Sources,
`j/k` or arrows to move, `Tab` to change focus, `[`/`]` to browse Agents,
`Enter` to switch/reapply providers, `s` to sync, `t` to test a source, `m` to
make it default, `w` to open the Wizard, `L` to change language, and `q` to
quit safely.

### Storage and results

Application configuration, progress state, staging, and durable backups live
under `~/.cc-switchy`. The compatible database, device-local settings, and
default Skills SSOT remain under `~/.cc-switch`; a configured
`~/.agents/skills` SSOT is respected.

- Exit code `0`: restore and all requested projections succeeded.
- Exit code `1`: configuration, transport, validation, restore, or rollback failed.
- Exit code `2`: database and Skills were restored, but at least one Agent projection warned.

The Agent capability matrix and TOML example in the Chinese section apply
equally to the English interface.

### Security limits

> The CC Switch snapshot format is not encrypted. `db.sql` may contain provider
> API keys, access tokens, and other plaintext secrets. Protect the WebDAV/S3
> account, storage, network path, local configuration, and backups accordingly.

`cc-switchy` exposes no remote write API. It validates manifest and artifact
limits, SHA-256 hashes, ZIP paths and expansion limits, and SQL imports before
live replacement. It uses Rustls with public webpki roots; endpoints signed only
by a private CA are not trusted unless that CA is available through a future
supported mechanism. For Linux portability, prefer the MUSL release; the GNU
binary inherits the build runner's glibc baseline.

## Compatibility and license

`cc-switchy` is MIT licensed. CC Switch compatibility behavior was studied and
adapted from [CC Switch](https://github.com/farion1231/cc-switch) at commit
`c6197ae32450cd70e2bf03b35e3f5f53ac12044c`. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for attribution and the
upstream MIT notice.
