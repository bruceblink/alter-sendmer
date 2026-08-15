# AlterSendme GPUI 开发计划

每个里程碑都是独立、可验证、可提交的小功能。完成一个功能后先执行对应验证，再提交并推送。

## M0: 工程与设计基线

- 新建独立 crate，锁定与 `flash-shot` 相同的 GPUI commit。
- 接入本地 `sendmer` 路径依赖、tokio runtime、统一错误和日志入口。
- 完成设计文档、README、窗口启动骨架。
- 验收：`cargo fmt --all -- --check`、`cargo check`、基础单测。

## M1: 原生工作台与静态状态机

- 实现 Send/Receive tabs、窗口标题区、页脚、主题切换、语言菜单。
- 实现发送路径卡片、接收 ticket 输入、状态和错误面板。
- 接入 GPUI 文本输入和真实 path prompt。
- 验收：状态机单测、`cargo test`、Windows 可见窗口截图。

## M2: 发送主线

- 文件/目录选择和 `ExternalPaths` 拖放。
- `sendmer::send` 真实启动、ticket 展示/复制、事件进度。
- 停止共享、`SendHandle` 生命周期和退出清理。
- 验收：无网络单测 + 两进程本地/relay 手工传输，确认 ticket 可被另一端使用。

## M3: 接收主线

- 保存目录选择、ticket 粘贴、`receive` 真实任务和 abort handle。
- 文件名事件、进度、完成摘要、取消和输出目录 reveal。
- 验收：sendmer 端到端测试复用、真实文件 hash 对比、失败后临时目录清理。

## M4: 1:1 细节收敛

- 对齐所有文案和 21 种语言的核心键，保留英文回退。
- 更新检查、赞助、窗口最小化/关闭，并确认 sendmer 自身负责临时目录清理。
- 完成无障碍标签、键盘焦点、窄窗口布局和 reduced-motion 等效行为。
- 验收：`cargo fmt --all -- --check`、`cargo clippy --all-targets`、`cargo test`、桌面和小窗口截图。

## M5: 发布与迁移

- 生成 Windows NSIS/portable、Linux AppImage/DEB 与 macOS app/DMG 原生包。
- 将默认依赖切到 crates.io 发布的 `sendmer = "0.7.0"`，通过 opaque `SendHandle` 复用核心关闭和清理路径，并通过干净检出构建。
- 发布前执行跨项目回归、资产检查和版本一致性校验。

## 工程化交付

- `.github/workflows/ci.yml` 在 Windows、Ubuntu 和 macOS 执行 fmt、check、Clippy 和 workspace tests。
- `.github/workflows/release.yml` 在三个原生 runner 上构建并签名安装包；手动运行可先只验证产物，不发布 Release。
- `scripts/write-release-manifest.ps1` 校验每个平台的安装包与 `.sig` 一一对应，再生成 `latest.json` 和 `SHA256SUMS`。
- `scripts/package-portable.ps1` 只负责 Windows 免安装包；其余原生格式统一由 cargo-packager 生成。
- NSIS 安装器默认按用户权限安装；portable、AppImage 和 DMG 均不会在安装前修改用户文件。

## 当前批次边界

M0-M5 已形成可运行的原生发送/接收主线；本批补齐了明确的六态失败/重试模型、目录进度、
历史持久化、主题/relay/重试偏好、票据保存/打开、诊断入口与本地化生成。在线更新依赖带
minisign 签名的 GitHub release manifest；跨平台安装与人工视觉回归仍需在发布流水线中单独
验证，不能由 `cargo test` 代替。

归档旧 Tauri 仓库前的界面收敛已将导航重组为主任务、次级工具和设置面板三个层级；语言下拉改为
顶层可滚动浮层，并为全部 21 种语言补齐应用外壳文案。Windows 发布验收使用
`scripts/capture-ui-acceptance.ps1` 固定输出五个关键状态截图，避免只凭单元测试判断布局完成。
