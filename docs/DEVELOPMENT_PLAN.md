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
- 停止共享、SendResult 生命周期和退出清理。
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

- 生成 Windows portable ZIP 与 Inno Setup 安装器，保留跨平台 Rust 构建入口。
- 将默认依赖切到公开 `sendmer` `v0.6.0` Git 标签，确保干净检出可构建。
- 发布前执行跨项目回归、资产检查和版本一致性校验。

## 工程化交付

- `.github/workflows/ci.yml` 在 Windows 上执行 fmt、check、clippy 和 workspace tests。
- `.github/workflows/release.yml` 在版本标签上构建 portable ZIP、SHA-256 校验文件和 Inno Setup 安装器。
- `scripts/write-release-manifest.ps1` 生成 `latest.json`，供 GPUI 客户端的检查更新入口读取。
- `scripts/package-portable.ps1` 与 `scripts/package-installer.ps1` 是本地与 CI 共用的唯一打包入口。
- 安装器默认按用户权限安装到 `{autopf}`，不会要求管理员权限；portable 包不写入安装目录外的数据。

## 当前批次边界

M0-M5 已形成可运行的原生发送/接收主线；本批补齐了明确的六态失败/重试模型、目录进度、
历史持久化、主题/relay/重试偏好、票据保存/打开、诊断入口与本地化生成。在线更新依赖
GitHub release manifest，跨平台人工视觉回归仍需在发布流水线中单独验证，不能由 `cargo test`
代替。
