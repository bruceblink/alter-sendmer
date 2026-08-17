# AlterSendmer GPUI 设计说明

## 1. 术语表与命名约定

| 规范名称 | English / 缩写 | 职责边界 | 不代表什么 |
| --- | --- | --- | --- |
| 核心传输层 | sendmer Core | 负责 ticket、点对点传输、限速、重试和资源清理 | 不负责 GPUI 状态或本地化 |
| 桌面客户端 | AlterSendmer Desktop | 负责配置、状态投影、历史和平台交互 | 不复制传输协议或 limiter |
| 传输适配器 | Transfer Adapter | 把桌面配置和事件映射到 sendmer 公开 API | 不是第二套传输实现 |
| 上传速率上限 | Upload Rate Limit | 一个 sender 对所有接收方共享的 payload 上限 | 不是每个 peer 的独立配额或精确 QoS |
| 持久接收缓存 | Persistent Receive Cache | 由核心传输层保存已验证接收数据，供失败或取消后的后续进程恢复 | 不是云端存储、永久传输会话或跨任意设备同步 |
| 事件信封 | Event Envelope | 承载 schema 版本、会话 ID、序号、阶段和事件载荷 | 不参与传输控制流或替代函数返回值 |
| 结构化错误 | Structured Transfer Error | 承载错误码、失败阶段、可重试属性和安全摘要 | 不是本地化文案或完整 anyhow 错误链 |

本文后续统一使用上述规范名称。核心传输层版本只能通过 crates.io 语义化版本依赖接入，
不得使用本地 path 或 Git revision 作为发布依赖。

公开产品名统一为 `AlterSendmer`。内部 crate/进程名 `alter-sendme-gpui`、安装标识
`top.likanug.alter-sendme` 和旧 `AlterSendme` 配置目录仅为升级兼容而保留，不作为界面或
发行资产名称展示。

## 2. 目标与边界

本项目是 `F:\project\alter-sendme` 的原生 GPUI 1:1 重写，不改变用户可见的产品主线：

- 发送文件或目录，支持系统文件选择器和拖放。
- 生成并复制 `sendmer receive ...` ticket，等待一个或多个接收方。
- 接收端粘贴 ticket，选择保存目录，显示文件名和实时进度。
- 发送/接收均可取消，完成后展示名称、大小、耗时和平均速度。
- 保留主题、语言、检查更新、赞助入口和窗口控制。

不在本次重写中复制 Tauri/WebView、React、Tailwind 或 Framer Motion。`sendmer` 是共享的传输
核心项目，GPUI 客户端只负责生命周期、状态投影和交互；协议、加密、NAT 穿透、relay 和路径
安全继续由 `F:\project\sendmer` 负责。

## 3. 总体架构

```text
GPUI Window
  -> AlterSendmeApp (唯一 UI 状态机)
     -> Transfer Adapter (异步 send/receive + cancel + options mapping)
        -> sendmer 0.9.0 (P2P/TLS/QUIC + Persistent Receive Cache)
     -> Preferences (relay/retry/chunk/upload limit/receive cache)
     -> Platform adapters (path prompt, clipboard, reveal, theme)
     -> Localization catalog (compile-time tables generated from the bundled locales directory)
```

### 3.1 单进程状态模型

`AlterSendmeApp` 持有当前 tab、发送状态、接收状态、ticket 输入、选择路径、提示信息和主题。
所有异步任务只通过 `WeakEntity` 回到 GPUI 主线程更新状态；任务完成前检查 generation，避免
旧传输覆盖新传输。

发送状态：`Idle -> Preparing -> Sharing -> Metadata -> Transporting -> Finalizing -> Completed`，
停止时进入 `Stopping -> Idle`；启动或传输失败进入 `Failed`，可通过 `Try again` 重置。接收状态：
`Idle -> Connecting -> Metadata -> Transporting -> Exporting -> Finalizing -> Completed`，取消回到
`Idle`，失败进入 `Failed` 并保留结构化错误摘要。Completed 提供 Done/New transfer、Open folder，
Failed 提供 Retry。

### 3.2 事件桥

实现 `sendmer::EventEmitter`，把 `TransferEventEnvelope` 转为无锁 `async_channel` 消息。适配器
先校验 schema 版本、会话 ID 和严格递增序号，再按 `TransferEventData` 投影 `Started`、
`Progress`、`FileNames`、`Completed`、`Failed` 和 `Cancelled`；事件 emitter 不阻塞网络任务，
丢失事件不会改变 sendmer 的错误控制流。

### 3.3 资源、取消与限速

- 发送端保存 opaque `SendHandle`，直到用户停止共享或应用退出；router、store、temp tag
  和临时目录的生命周期由 sendmer `0.9.0` 内部管理。
- 接收端保存 watch cancellation sender，取消按钮发出优雅取消信号；sendmer 负责关闭 endpoint、
  store 和导出临时资源，任务收尾后 GPUI 再统一清除状态。启用持久接收缓存时，失败或取消会
  保留已验证数据供后续进程恢复；成功导出后对应缓存条目由核心传输层删除。
- 应用退出先停止发送 router，再等待接收任务结束。
- 上传上限以 MiB/s 输入并持久化，传输适配器使用 checked multiplication 转换为
  `SendOptions::max_upload_rate_bytes_per_sec`；`None` 表示无限制。实际 limiter 只存在于核心传输层。
- 持久接收缓存默认启用，新条目默认 TTL 为 7 天。桌面客户端只映射启用开关和 `1 / 7 / 30`
  天选项，不自行读写缓存内容；TTL 变更只用于之后创建的新条目，不重写既有条目。
- 自动清理和用户触发的清理都调用核心传输层能力，只删除 schema 已知、已过期且当前非活动的
  条目；活动条目、未知 schema 条目和未过期条目必须保留。

## 4. GPUI 映射

| 原 Tauri/React 能力 | GPUI 实现 |
| --- | --- |
| React `App` / hooks | `AlterSendmeApp` entity + Rust 方法 |
| Tauri `invoke` | 传输适配器的 tokio 任务 |
| Tauri event | `async_channel::Sender/Receiver<UiEvent>` |
| `dialog.open` | `Window::prompt_for_paths` |
| Tauri drag-drop | `ExternalPaths` + `.on_drop` |
| `navigator.clipboard` | `App::write_to_clipboard(ClipboardItem::new_string)` |
| opener reveal | `App::reveal_path` |
| CSS theme variables | `Theme` + GPUI palette, with `WindowAppearance` for System mode |
| i18next catalog | `Locale` + generated `common.json` lookup, missing keys fall back to English |

票据输入使用 GPUI 原生 `ElementInputHandler`，因此支持中文输入法、选择、粘贴和键盘事件；不
通过伪造的按钮或不可编辑标签替代真实文本框。

传输历史保存在系统应用数据目录的 `history.json`，只写入角色、路径、大小、耗时、平均速度、
时间、结果和发送端票据；不会写入文件内容。主题选择保存在同一应用配置目录，relay 模式和接收
重试/块大小在工作台偏好控件中切换；上传上限支持“不限速”或 `1..10240 MiB/s` 整数，持久接收
缓存支持关闭或为新条目选择 `1 / 7 / 30` 天 TTL，所有配置均持久化并传给核心传输层。

## 5. 视觉与交互

- 1024x720 起始窗口，最小 760x560，可调整大小；内容区在较小窗口中独立滚动并保留底部安全间距。
- 深色默认、浅色可切换；主色为绿色，传输态使用蓝色，错误使用红色。
- 应用窗口和安装快捷方式使用统一的 AlterSendmer 传输标记；标记 SVG 编译期内嵌，Windows ICO 嵌入可执行文件。
- 内容列宽限制为 720px，发送和接收共享同一工作台布局。
- 发送/接收是唯一的主任务分段导航；历史、传输设置和诊断是低强调的次级工具，不再与传输参数混排为一组主按钮。
- 接收页按“传输票据、保存位置、开始接收”形成单向操作流；粘贴、浏览和使用帮助保持次级样式，失败重试复用唯一主按钮。
- 接收页的空闲态采用三步工作流：`1` 票据输入、`2` 保存位置、`3` 开始接收；票据输入和保存位置各只有一个明确的主控件，粘贴与浏览是同一行的次级动作，帮助入口放在操作区下方，不与主按钮竞争。
- 接收页不再把“在此处粘贴票据”、票据输入和“从剪贴板粘贴”渲染成三个同级动作；长票据在输入框内截断显示但仍保留原生文本选择与编辑能力，保存路径单独显示并允许在未开始传输前重新选择。
- 接收主按钮始终是唯一的宽按钮：无票据时禁用并保持布局稳定，失败时原位变为“重试”，传输中替换为停止按钮；帮助内容只在用户展开后插入，不改变输入控件的语义。
- relay、重试、下载分块和上传上限集中在可展开的传输设置面板；上传上限使用模式按钮与
  固定宽度数字输入，不用文字卡片模拟数值控件。
- 语言选择器是最后绘制的顶层浮层，始终覆盖而不是被内容区遮挡；当前语言固定在第一项，完整 21 项列表独立滚动。
- 主要操作是带文字的命令按钮，主题和语言使用紧凑控件；固定高度、最小宽度和换行规则防止长翻译改变布局。
- 任何传输进行中都禁用 tab 切换和会改变资源语义的选择动作。

## 6. 可观测性与错误

用户可见错误由 sendmer `TransferError` 提供稳定错误码、阶段、可重试属性和安全摘要；桌面端只
本地化摘要，不展示完整 anyhow 错误链。日志使用 `tracing`/`log` 记录 ticket 生命周期和资源
清理，不记录完整文件内容或私密 ticket 到持久日志。历史只增加可选 session ID、错误码和失败
阶段字段，并继续兼容旧 `history.json`。

## 7. 本地化与可访问性

构建脚本读取项目内 `locales/<locale>/common.json`，在 `OUT_DIR` 生成只读 Rust 查找表；
因此发布二进制不依赖运行时 JSON 文件。语言资源从原项目同步后随 GPUI 项目一起发布。GPUI 的主要动作、标签页、
拖放区和 ticket 输入均声明了 AccessKit role 与 aria label，便于键盘和辅助技术识别。
主题、语言、历史、传输设置、诊断、状态和更新入口的应用外壳文案在全部 21 个 locale 中显式定义；测试会拒绝
这些导航键回退为英文。`scripts/capture-ui-acceptance.ps1` 通过 UI Automation 核对下拉框暴露 21 个选项，
并用 DWM `PrintWindow` 生成语言浮层，以及默认和最小窗口尺寸的发送页、接收页和设置页截图。

## 8. 跨平台发布与更新

Windows x86_64 发布 NSIS 安装器和 portable ZIP，Linux x86_64 发布 AppImage 与 DEB，macOS
Apple Silicon 发布 app 更新归档和 DMG。三类更新归档由 cargo-packager 使用同一 minisign
私钥签名；`latest.json` 按 `OS-ARCH` 映射下载地址、签名和安装格式。客户端只在签名验证通过后
调用系统对应安装路径，启动时的检查保持静默，用户主动检查时才显示“已是最新”或错误。

发送和接收协议行为由 `sendmer` 的跨平台测试负责；GPUI 客户端测试聚焦状态机、格式化、事件
归并和 generation 防竞态。CI 在 Windows、Ubuntu 和 macOS 分别执行 fmt、check、Clippy 与测试，
发布工作流还必须在三个原生 runner 上实际产出安装包，避免用交叉编译代替平台打包验收。
进程中断与重启测试只能证明确定性的连接恢复和缓存复用；除非另有独立测试设施，不得据此宣称
已完成内核级丢包/时延故障注入，也不得把持久接收缓存描述为完整的跨设备永久续传。
