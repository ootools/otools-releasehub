# ReleaseHub · 追新 — OTools 独立插件

将 [huai-bot/releasehub](https://github.com/huai-bot/releasehub) 大部分非 AI 功能移植到 OTools Panel。保留 Svelte 5 界面和 Rust/SQLite 原生核心，不嵌入独立 EXE，不要求注册或授权才能追踪公开软件。

## 功能

- 软件库、搜索与过滤、正式/预览频道、集中检查更新、发布说明与 Markdown 安全渲染。
- 每个软件的安装包偏好、文件名规则、手选附件；按 Windows/macOS/Linux 和 x64/arm64 保守推荐。
- 后台下载、实时进度、取消/重试、原作者 SHA-256 校验与计算摘要区分。
- 确认后将 ZIP/上游已知便携单文件准备到隔离受管目录，保留旧版本与配置；手动记录本机版本与安全切换受管版本。
- 安装器只在明确确认后打开系统安装界面，不宣称安装已完成；非 ZIP 便携格式可下载，在系统中处理，不自动解包/执行。
- GitHub 搜索、Stars、自己的仓库（含授权的私有仓库）、README 与详情、近期动态、确认后 Star/取消 Star 和修改说明/主页。私有仓库不加入发行追踪。
- 浅色/深色/跟随系统、自动检查、并行下载上限、桌面通知、订阅导入/导出。

移除 AI 连接、Agent/MCP/构建任务、独立应用托盘与退出行为。后台任务随 OTools 进程运行，不依赖 Panel 可见性。数据在宿主提供的插件数据目录中；账号不随订阅导出迁移。

## GitHub 登录

1. 首选 **登录 GitHub**：使用每个平台包附带的官方 GitHub CLI 2.100.0，在浏览器输入短码；插件配置隔离，不继承开发者环境变量、PATH 上的其他 gh 或已有工具配置。系统 GitHub CLI 凭据可能按账号共享，但“仅断开追新”不注销其他工具。
2. **令牌登录**：你提供自己的 GitHub PAT，先验证账号，保存在本插件作用域的系统凭据条目。界面、日志、SQLite 和订阅导出不保存令牌。
3. **外部浏览器授权**：可选，你配置可信 HTTPS OAuth 授权服务。服务接受 `redirect_uri` 与 `state`，完成后返回 `otools://plugin/otools-releasehub/callback/github-login?...&token=...`。需要支持安全回调的宿主版本。此仓库不包含服务端 OAuth secret，也不假装提供公用授权服务。

macOS 用 Keychain，Windows 用凭据管理器，Linux 用 Secret Service；若安全存储不可用，不将 Token 降级写到普通文件。公共查询仍可匿名使用。设备授权依赖官方 CLI 的安全存储支持。

## 安装与开发

从 [Releases](https://github.com/ootools/otools-releasehub/releases) 下载与你的系统/架构一致的 `.oplg`，在 OTools 插件市场使用本地安装。macOS arm64 与 Intel 是不同的包。

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test
cargo test --locked --manifest-path native/Cargo.toml
node scripts/prepare-github-cli.mjs --platform macos --arch arm64
pnpm build
node scripts/pack-plugin.mjs --platform macos --arch arm64
pnpm verify
pnpm dev # 127.0.0.1:6567，仅用于 OTools dev plugin
```

仓库可独立 clone/build，不要求相邻 xyito/OTools 仓库。浏览器直开只有显式界面预览，不将演示数据冒充真实下载/检查。

## 回调安全

宿主原有 `otools://` 注册扩展为插件参数路由、Panel 激活和短期、随机、单次使用 state。参数留在内存 mailbox，按真实 webview/文档/插件来源身份定向取件，不拼进插件导航 URL 或广播 Token。普通打开链接可冷启动宿主；宿主进程重启后旧授权 state 无效，需重试。完整约定见 [docs/CALLBACKS.md](docs/CALLBACKS.md)。

## 许可

MIT；保留原作者 Copyright。详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
