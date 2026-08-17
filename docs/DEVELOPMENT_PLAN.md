# AlterSendmer GPUI 开发计划

每个里程碑都是独立、可验证、可提交的小功能。完成一个功能后先执行对应验证，再提交并推送。

## 术语与仓库边界

| 规范名称 | English | 职责 | 不代表什么 |
| --- | --- | --- | --- |
| 核心传输层 | sendmer Core | 维护协议、传输选项、限速与清理契约 | 不负责 GPUI 状态或本地化 |
| 桌面客户端 | AlterSendmer Desktop | 消费正式 sendmer 版本，维护 GPUI 交互与平台能力 | 不复制传输协议或缓存实现 |
| 传输适配器 | Transfer Adapter | 在两者之间映射配置、事件和生命周期 | 不是第二套传输实现 |
| 持久接收缓存 | Persistent Receive Cache | 由核心传输层保留已验证数据，支持失败或取消后的后续进程恢复 | 不是云存储、永久会话或跨设备同步 |
| 事件信封 | Event Envelope | 携带 sendmer schema 版本、会话 ID、序号、阶段和事件载荷 | 不替代传输控制流或函数返回值 |
| 结构化错误 | Structured Transfer Error | 携带稳定错误码、阶段、可重试属性和安全摘要 | 不是本地化文案或完整错误链 |

桌面客户端不得复制核心传输逻辑，也不得使用 Git revision 绕过 sendmer 的正式版本发布顺序。

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
- 验收：sendmer 端到端测试复用、真实文件 hash 对比、失败后导出临时资源清理；持久缓存保留策略由 M9 单独验收。

## M4: 1:1 细节收敛

- 对齐所有文案和 21 种语言的核心键，保留英文回退。
- 更新检查、赞助、窗口最小化/关闭，并确认 sendmer 自身负责临时资源清理与缓存生命周期。
- 完成无障碍标签、键盘焦点、窄窗口布局和 reduced-motion 等效行为。
- 验收：`cargo fmt --all -- --check`、`cargo clippy --all-targets`、`cargo test`、桌面和小窗口截图。

## M5: 发布与迁移

- 生成 Windows NSIS/portable、Linux AppImage/DEB 与 macOS app/DMG 原生包。
- 将默认依赖切到 crates.io 发布的 sendmer 语义化版本，通过 opaque `SendHandle` 复用核心关闭和清理路径，并通过干净检出构建。
- 发布前执行跨项目回归、资产检查和版本一致性校验。

## M6: v0.3.0 上传速率配置闭环（已完成）

- 在传输设置中提供“不限速”或 `1..10240 MiB/s` 自定义发送上限。
- 重组接收页操作层级，消除重复票据提示和竞争主按钮，并覆盖默认与最小窗口视觉验收。
- 兼容旧 `preferences.json`，持久化有效设置并拒绝零值、非整数和溢出值。
- 由传输适配器转换为 sendmer bytes/s 选项；桌面客户端不实现第二套 limiter。
- 补齐 21 种语言、配置迁移、参数映射和 GPUI 输入回归。
- 验收：完整 fmt/check/Clippy/tests、Windows 默认与最小窗口设置页截图、`v0.3.0` 发布资产。

## M7: v0.4.0 版本化事件消费（已完成）

- 依赖正式发布的 sendmer 语义化版本，不使用本地 path 或 Git revision；当前 `0.9.0` 基线由 M9 接入。
- 使用事件信封的 schema 版本、会话 ID、严格序号和核心阶段驱动 UI 状态机。
- 按结构化错误码、阶段和可重试属性展示安全摘要，并在历史记录中保存可选诊断字段。
- 保持现有 `history.json` 兼容，不记录完整 ticket 或绝对路径到诊断信息。
- 验收：跨项目 contract tests、状态/错误 UI 截图和三平台发布门禁。

## 工程化交付

- `.github/workflows/ci.yml` 在 Windows、Ubuntu 和 macOS 执行 fmt、check、Clippy 和 workspace tests。
- `.github/workflows/release.yml` 在三个原生 runner 上构建并签名安装包；手动运行可先只验证产物，不发布 Release。
- `scripts/write-release-manifest.ps1` 校验每个平台的安装包与 `.sig` 一一对应，再生成 `latest.json` 和 `SHA256SUMS`。
- `scripts/package-portable.ps1` 只负责 Windows 免安装包；其余原生格式统一由 cargo-packager 生成。
- NSIS 安装器默认按用户权限安装；portable、AppImage 和 DMG 均不会在安装前修改用户文件。

## 当前批次边界

M0-M8 已形成可运行的原生发送/接收主线；当前版本补齐了明确的失败/重试模型、目录进度、
历史持久化、主题/relay/重试偏好、票据保存/打开、诊断入口与本地化生成。在线更新依赖带
minisign 签名的 GitHub release manifest；跨平台安装与人工视觉回归仍需在发布流水线中单独
验证，不能由 `cargo test` 代替。

M6 已完成并纳入 `v0.3.0`，M7 已完成并纳入 `v0.4.0`，M8 的接收工作流视觉修正已经完成，
M9 已接入正式发布的 sendmer `0.9.0` 持久接收缓存，并纳入 `v0.5.0`。该能力提供受约束的跨进程
恢复，不包含账号、云端存储、后台同步服务、内核级网络故障注入或完整的跨设备永久续传。

## M8: 接收工作流视觉修正（已完成）

- 将接收空闲态收敛为“票据 -> 保存位置 -> 开始接收”的三步操作流。
- 移除重复的票据粘贴提示和与主操作同权重的帮助按钮；输入、粘贴、浏览和重试保持清晰的主次关系。
- 保持 GPUI 原生票据输入、键盘粘贴、失败重试、取消和核心事件状态机不变，只调整视图组合与稳定尺寸。
- 验收：`cargo fmt --all -- --check`、`cargo clippy --all-targets`、`cargo test`，并在 1024x720 与 760x560 窗口截图中确认无重叠、长票据不撑破布局、禁用态和失败重试态可识别。

## M9: v0.5.0 sendmer 0.9.0 持久接收缓存集成（已完成）

- 将传输适配器升级到 crates.io 已发布的 `sendmer = "0.9.0"`，保持干净检出不依赖 sibling path 或 Git revision。
- 默认启用持久接收缓存：失败或取消后保留已验证数据，允许使用仍然有效的 ticket 和发送源在后续进程中恢复；成功导出后删除对应缓存条目。
- 在传输设置中提供缓存启用开关，并允许为新条目选择 `1 / 7 / 30` 天 TTL；默认值为 7 天，旧配置缺少字段时迁移到该默认值。
- 清理动作只委托给核心传输层，并只删除 schema 已知、已过期且当前非活动的条目；活动、未知 schema 与未过期条目必须保留。
- UI 只展示缓存配置、恢复状态和安全错误摘要，不直接操作 blob/manifest，也不把缓存表述为云同步或永久传输会话。

完成目标与验收：

- 配置迁移和传输适配器映射测试覆盖默认启用、关闭以及 `1 / 7 / 30` 天 TTL，并确认 TTL 变更只影响新条目。
- sendmer 核心的进程级端到端测试覆盖接收失败或取消后的重新启动、同一 ticket/发送源恢复、输出内容一致，以及成功导出后缓存删除；桌面端适配器测试验证启用缓存后的真实双进程传输，不复制核心恢复状态机。
- 缓存清理测试确认只删除过期且非活动的已知条目，并保留活动、未知 schema 和未过期条目。
- 运行完整 fmt/check/Clippy/workspace tests，并在 1024x720 与 760x560 截图中检查缓存控件、禁用态、长翻译和状态反馈无重叠。
- 将上述验证限定为应用级进程恢复与缓存复用；没有独立证据时，不宣称内核丢包/时延注入或完整跨设备永久续传。

归档旧 Tauri 仓库前的界面收敛已将导航重组为主任务、次级工具和设置面板三个层级；语言下拉改为
顶层可滚动浮层，并为全部 21 种语言补齐应用外壳文案。Windows 发布验收使用
`scripts/capture-ui-acceptance.ps1` 固定输出七个关键状态截图，避免只凭单元测试判断布局完成。
