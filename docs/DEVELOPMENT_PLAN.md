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
- 更新检查、赞助、窗口最小化/关闭、启动孤儿目录清理。
- 完成无障碍标签、键盘焦点、窄窗口布局和 reduced-motion 等效行为。
- 验收：`cargo fmt --all -- --check`、`cargo clippy --all-targets`、`cargo test`、桌面和小窗口截图。

## M5: 发布与迁移

- 生成 Windows/macOS/Linux 打包配置，移除旧 Tauri 构建依赖。
- 将默认依赖切到发布版 `sendmer`，保留本地联调 profile。
- 发布前执行跨项目回归、资产检查和版本一致性校验。

## 当前批次边界

M0-M3 已形成可运行的原生发送/接收主线；本批补齐了 M4 的本地化生成、System 主题跟随和
基础 AccessKit 标注。M4 剩余的在线更新安装、完整打包矩阵和跨平台人工视觉回归仍需在发布
流水线中单独验证，不能由 `cargo test` 代替。
