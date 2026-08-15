# AlterSendme GPUI 设计说明

## 1. 目标与边界

本项目是 `F:\project\alter-sendme` 的原生 GPUI 1:1 重写，不改变用户可见的产品主线：

- 发送文件或目录，支持系统文件选择器和拖放。
- 生成并复制 `sendmer receive ...` ticket，等待一个或多个接收方。
- 接收端粘贴 ticket，选择保存目录，显示文件名和实时进度。
- 发送/接收均可取消，完成后展示名称、大小、耗时和平均速度。
- 保留主题、语言、检查更新、赞助入口和窗口控制。

不在本次重写中复制 Tauri/WebView、React、Tailwind 或 Framer Motion。`sendmer` 是共享的传输
核心项目，GPUI 客户端只负责生命周期、状态投影和交互；协议、加密、NAT 穿透、relay 和路径
安全继续由 `F:\project\sendmer` 负责。

## 2. 总体架构

```text
GPUI Window
  -> AlterSendmeApp (唯一 UI 状态机)
     -> TransferController (异步 send/receive + cancel)
        -> sendmer::{send, receive} (P2P/TLS/QUIC)
     -> Platform adapters (path prompt, clipboard, reveal, theme)
     -> Localization catalog (compile-time tables generated from the bundled locales directory)
```

### 2.1 单进程状态模型

`AlterSendmeApp` 持有当前 tab、发送状态、接收状态、ticket 输入、选择路径、提示信息和主题。
所有异步任务只通过 `WeakEntity` 回到 GPUI 主线程更新状态；任务完成前检查 generation，避免
旧传输覆盖新传输。

发送状态：`Idle -> Preparing -> Sharing -> Transporting -> Completed`，停止时进入
`Stopping -> Idle`；启动或传输失败进入 `Failed`，可通过 `Try again` 重置。接收状态：
`Idle -> Connecting -> Transporting -> Completed`，取消回到 `Idle`，失败进入 `Failed` 并保留
可读错误。Completed 提供 Done/New transfer、Open folder，Failed 提供 Retry。

### 2.2 事件桥

实现 `sendmer::EventEmitter`，把 `TransferEvent` 转为无锁 `async_channel` 消息。`Started`、
`Progress`、`FileNames`、`Completed` 和 `Failed` 均在 UI 线程消费；事件 emitter 不阻塞网络
任务，丢失事件不会改变 sendmer 的错误控制流。

### 2.3 资源与取消

- 发送端保存 opaque `SendHandle`，直到用户停止共享或应用退出；router、store、temp tag
  和临时目录的生命周期由 sendmer `0.7.0` 内部管理。
- 接收端保存 watch cancellation sender，取消按钮发出优雅取消信号；sendmer 负责关闭 endpoint、
  store 和临时目录，任务收尾后 GPUI 再统一清除状态。
- 应用退出先停止发送 router，再等待接收任务结束。

## 3. GPUI 映射

| 原 Tauri/React 能力 | GPUI 实现 |
| --- | --- |
| React `App` / hooks | `AlterSendmeApp` entity + Rust 方法 |
| Tauri `invoke` | `TransferController` 的 tokio 任务 |
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
重试/块大小在工作台偏好控件中切换并传给 sendmer。

## 4. 视觉与交互

- 1024x720 起始窗口，最小 760x560，可调整大小；内容区在较小窗口中独立滚动并保留底部安全间距。
- 深色默认、浅色可切换；主色为绿色，传输态使用蓝色，错误使用红色。
- 应用窗口和安装快捷方式使用统一的 AlterSendme 传输标记；标记 SVG 编译期内嵌，Windows ICO 嵌入可执行文件。
- 内容列宽限制为 720px，发送和接收共享同一工作台布局。
- 发送/接收是唯一的主任务分段导航；历史、传输设置和诊断是低强调的次级工具，不再与传输参数混排为一组主按钮。
- relay、重试和下载分块集中在可展开的传输设置面板，每个设置同时显示本地化名称与当前值。
- 语言选择器是最后绘制的顶层浮层，始终覆盖而不是被内容区遮挡；当前语言固定在第一项，完整 21 项列表独立滚动。
- 主要操作是带文字的命令按钮，主题和语言使用紧凑控件；固定高度、最小宽度和换行规则防止长翻译改变布局。
- 任何传输进行中都禁用 tab 切换和会改变资源语义的选择动作。

## 5. 可观测性与错误

用户可见错误必须包含阶段（准备、共享、连接、接收）和底层错误消息；日志使用
`tracing`/`log` 记录 ticket 生命周期和资源清理，不记录完整文件内容或私密 ticket 到持久日志。

## 6. 本地化与可访问性

构建脚本读取项目内 `locales/<locale>/common.json`，在 `OUT_DIR` 生成只读 Rust 查找表；
因此发布二进制不依赖运行时 JSON 文件。语言资源从原项目同步后随 GPUI 项目一起发布。GPUI 的主要动作、标签页、
拖放区和 ticket 输入均声明了 AccessKit role 与 aria label，便于键盘和辅助技术识别。
主题、语言、历史、传输设置、诊断、状态和更新入口的应用外壳文案在全部 21 个 locale 中显式定义；测试会拒绝
这些导航键回退为英文。`scripts/capture-ui-acceptance.ps1` 通过 UI Automation 核对下拉框暴露 21 个选项，
并用 DWM `PrintWindow` 生成默认页、设置页、接收页、语言浮层和最小窗口截图。

## 7. 跨平台发布与更新

Windows x86_64 发布 NSIS 安装器和 portable ZIP，Linux x86_64 发布 AppImage 与 DEB，macOS
Apple Silicon 发布 app 更新归档和 DMG。三类更新归档由 cargo-packager 使用同一 minisign
私钥签名；`latest.json` 按 `OS-ARCH` 映射下载地址、签名和安装格式。客户端只在签名验证通过后
调用系统对应安装路径，启动时的检查保持静默，用户主动检查时才显示“已是最新”或错误。

发送和接收协议行为由 `sendmer` 的跨平台测试负责；GPUI 客户端测试聚焦状态机、格式化、事件
归并和 generation 防竞态。CI 在 Windows、Ubuntu 和 macOS 分别执行 fmt、check、Clippy 与测试，
发布工作流还必须在三个原生 runner 上实际产出安装包，避免用交叉编译代替平台打包验收。
