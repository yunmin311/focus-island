# 通知与邮箱验收

本次 Windows 验证日期为 2026 年 11 月 13 日，时区 UTC+08:00。证据分为真实邮箱登录、用户桌面实测、Windows 原生自动测试和 Chromium 页面模拟，不互相代替。22:15 完成 QQ 真实凭据登录，22:17 新版重启后进入 IDLE 监听；用户随后确认收到新邮件弹窗并点击打开 QQ 邮箱网页。163 按用户本轮反馈记为此前真实测试通过，未将这一反馈扩展为下面每一项都通过。

## Windows 系统通知可行性

FlowIsle 的「专注时静默」只能静默本应用，不能屏蔽其他应用的系统 Toast。专注计时运行且打开静默时，FlowIsle 暂停邮件和系统镜像通知的岛内提示及新增记录。任务提醒、番茄完成和休息结束仍按原有设置工作。「打开设置」交由用户在 Windows 中管理系统通知。应用没有写入 Windows 勿扰状态或通知策略。

| 路径 | 接口与权限 | 验证结果 |
| --- | --- | --- |
| UserNotificationListener | WinRT 监听和读取用户通知，也支持移除已送达的通知。微软的打包应用流程要求声明 `userNotificationListener` 能力，并在 UI 线程请求用户访问权限。 | 本机 Windows 11 26200 的无包身份探测进程可读取访问状态，返回 `Allowed`。接口没有投递前拦截或全局静默控制。 [微软通知监听器文档](https://learn.microsoft.com/en-us/windows/uwp/design/shell/tiles-and-notifications/notification-listener) |
| Focus Sessions / 勿扰 | `FocusSessionManager.IsFocusActive` 可以读取状态。程序启动或关闭专注需要微软批准的 Limited Access Feature 和令牌，用户在设置中启用勿扰是另一条路径。 | 本机 WinRT 类型存在，读取结果为 `False`。关闭接口会结束用户的所有专注会话，文档没有给普通 Win32 应用提供可独立撤销的勿扰租约。 [Focus Sessions](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/applifecycle/focus-session?tabs=csharp) · [DeactivateFocus](https://learn.microsoft.com/en-us/uwp/api/windows.ui.shell.focussessionmanager.deactivatefocus) |
| 通知策略 / 注册表 | `NoToastNotification` 属于管理员策略，用户级策略影响该用户的系统通知。CloudStore 中的私有状态缺少受支持的第三方勿扰写入接口。 | 只读取本机用户策略键，`NoToastNotification` 未设置。系统级策略和私有状态均无法为应用提供崩溃自动恢复及并发修改隔离。 [ADMX_WPN](https://learn.microsoft.com/en-us/windows/client-management/mdm/policy-csp-admx-wpn) |
| Shell 消息 | `SHQueryUserNotificationState` 查询系统是否适合显示通知，`WM_SETTINGCHANGE` 广播设置变化。Win32 查询不需要应用包身份。 | 本机查询返回 `S_OK`、状态枚举 5。这两个接口都没有拦截 Toast 或切换勿扰的操作。 [SHQueryUserNotificationState](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shqueryusernotificationstate) · [WM_SETTINGCHANGE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-settingchange) |
| ToastNotificationManager | 管理应用身份所拥有的 Toast。桌面 Toast 的注册与身份要求取决于应用模型。 | 官方接口范围限于对应应用，缺少第三方全局屏蔽。 [ToastNotificationManager](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotificationmanager) |
| Windows 设置 | `ms-settings:quiethours` 启动系统设置，是否更改由用户决定。 | Chromium 页面模拟已验证入口和 IPC 调用。本机 Windows 入口实际打开情况待交互验收。 [Windows 设置 URI](https://learn.microsoft.com/windows/uwp/launch-resume/launch-settings-app) |

结论是当前项目使用的公开 Windows 接口无法提供面向 Windows 10/11 的可靠、可恢复的全局 Toast 屏蔽。验证没有启用或关闭 Windows 勿扰。

## 邮箱矩阵

| 服务商 | 测试项 | 结果 | 证据（北京时间） |
| --- | --- | --- | --- |
| QQ | 公共端点 TLS、登录前 CAPABILITY 和 ID；真实账户全链路 | Windows TLS 与 CAPABILITY 成功，登录前列出 IDLE。公开端点返回 `* ID NIL`。22:15 真实登录、重启监听成功；用户确认真邮件弹窗点击打开 QQ 网页，详见追加矩阵。 | `windows-imap-public-final.log`，15:59:25；`probe-qq.log` |
| 163 | 公共端点 TLS、登录前 CAPABILITY 和 ID；真实账户全链路 | Windows 默认 TLS 握手遇到提前关闭。证书与主机名校验保持开启，增加 TLS 1.2 重试后，Windows TLS 与 CAPABILITY 成功。登录前未列出 IDLE，真实 Coremail ID 响应脱敏后回放通过。用户确认此前账户测试通过；未补录其原始逐项证据。 | `windows-imap-tls-versions.log`，15:58:34；`windows-imap-public-final.log`，15:59:25；`imap-163-public.imap`；原生测试日志 |
| Gmail | 公共端点 TLS、登录前 CAPABILITY；真实账户全链路 | Windows TLS 与 CAPABILITY 成功，登录前列出 IDLE。应用专用密码取决于账号是否符合 Google 要求，账户测试待做。 | `windows-imap-public-final.log`，15:59:25。 [Google 应用专用密码](https://support.google.com/mail/answer/185833?hl=en) |
| 126 | 公共端点 TLS、登录前 CAPABILITY；真实账户全链路 | Windows TLS 与 CAPABILITY 成功，登录前未列出 IDLE。账户测试待做。 | `windows-imap-public-final.log`，15:59:25 |
| Outlook.com | 公共端点探测、网页收件箱和授权方式 | 本轮公共端点探测和复试均未完成 TLS 握手。网页收件箱映射通过页面模拟。Outlook.com 要求 OAuth2，目前没有此认证流程。 | `windows-imap-public-final.log`，15:59:25；页面模拟日志。 [Microsoft 邮件客户端认证要求](https://support.microsoft.com/en-us/support/known-issues/modern-authentication-methods-now-needed-to-continue-syncing-outlook-email-in-non-microsoft-email-ap) |
| 自定义 | TLS 主机名校验和通知跳转；真实自定义邮箱 | 合法主机名和畸形输入通过原生测试，通知跳转回记录通过页面模拟。指定自定义服务器的真实登录和收信待做。 | 原生测试和页面模拟日志 |

| 用例 | 自动化证据 | Windows 真实验收 |
| --- | --- | --- |
| 测试连接、保存、清除授权码 | 原生回放覆盖登录和未读命令。Windows 独立探测使用临时目标验证应用凭据函数的保存、读取、清除及重复清除，15:59:22。 | 设置页使用测试账号连接，保存，退出重开设置确认已保存，再清除并检查状态。QQ 连接/保存/开启提醒由用户确认成功，22:15:49 又用已保存凭据完成原生登录验证；163 用户确认此前通过。清除操作的真实账号界面验收及 Gmail 实测仍待做。 |
| 错误、过期或撤销授权码 | 原生测试覆盖认证拒绝、脱敏文案和常见服务商提示。 | 使用虚构错误码和已撤销的专用测试码分别连接；核对失败提示和有效码可重试。服务端通常用同一类认证拒绝，界面提示检查或重新生成授权码。 |
| 自定义 IMAP host、TLS 错误 | 原生测试覆盖 host 校验。Windows 独立探测通过应用的 Schannel TLS 连接函数连接本地自签名测试端点，得到证书拒绝和预期中文提示，15:59:22。OpenSSL 的 QQ 错误主机名负例见 `tls-negative.log`，15:46:50。 | 配置自有测试 IMAP 服务器验证 993 TLS，分别用有效证书和测试服务器的无效证书。有效证书下的自定义邮箱登录和设置页错误显示待实测。 |
| 真实收信、IDLE 与轮询 | 原生回放覆盖 IDLE 通知、无 IDLE 和明确拒绝 IDLE 时的轮询。 | 启用监听后等诊断日志出现 `listening IDLE` 或 `listening poll`，再用第二账号发送测试邮件。服务器登录前的能力和实际选用模式可能不同。 |
| 重复抑制、MIME 发件人和主题 | 原生回放覆盖 MIME 解码及同一 UID 重复；页面模拟覆盖持久记录去重。 | 发送一封中文发件人、中文主题的测试邮件，等待多个监听周期，检查通知中心只新增一条且内容正确。 |
| 专注时静默、通知中心、岛内 peek | Chromium 模拟覆盖专注时系统与邮件记录和提示的抑制、peek 定位和 DOM 焦点。 | 先在普通状态收信并点击 peek，确认直接打开对应邮箱网页；运行专注计时且打开静默再发送一封，检查岛内和通知中心；退出专注后发第三封确认恢复。 |
| 应用重启后继续监听 | 快捷方式启动 Windows release 已验证。邮箱重新登录待验证。 | 保持专用码已保存，完整退出应用，用桌面快捷方式重开，等日志恢复监听，再发新邮件。监听以首次搜索到的未读为基线，停机时收到的邮件不会补发。 |

测试连接使用输入框内的授权码，保存操作才会写入 Windows 凭据管理器 `FocusIsland/Email`。本地只有一条监听凭据，按服务商轮测前需切换设置并保存对应专用测试码。查看诊断可读 `%APPDATA%\com.focus-island.desktop\focus-island\email-status.log`。诊断包含时间、服务商、监听模式和状态。验收记录请只保留这些字段和结果，避开邮箱地址、授权码、通知正文及应用标识符。

## 通知跳转手动步骤

| 通知 | Windows 桌面手动步骤 | 当前结果 |
| --- | --- | --- |
| 系统通知 | 允许镜像权限，用测试应用产生 Toast，打开通知中心点记录。检查来源应用前台。再用已卸载或已取消注册的测试来源检查可见错误、面板回到通知中心、焦点回到同一条记录。 | Chromium 模拟检查成功 IPC 和拒绝后的提示及焦点。Windows 原生测试拒绝空来源和未知来源。真实来源启动与面板前台焦点待手动验证。 |
| 任务提醒 | 建一个带提醒的临时任务，收到提醒后点击记录。检查具体任务详情。删除测试任务再点击历史记录，检查待办页。 | Chromium 模拟已验证对应详情、待办回退及 DOM 焦点。桌面操作待验证。 |
| 番茄完成 / 休息结束 | 让测试计时完整经过两个阶段，分别点两条通知，检查首页计时区获得焦点。 | Chromium 模拟已验证两条路由和 DOM 焦点。桌面操作待验证。 |
| QQ、163、126、Gmail、Outlook 邮件 | 点击对应服务商的邮件记录，确认默认浏览器打开该服务商收件箱。 | Chromium 模拟已验证五个固定 HTTPS URL。默认浏览器实际打开待验证。 |
| 自定义邮箱 / 旧邮件记录 | 点击没有可确定网页收件箱的记录，检查通知中心选中并聚焦该记录。 | Chromium 模拟已验证自定义邮件的记录焦点。桌面及旧记录待验证。 |
| 岛内 peek | 面板关闭时点击邮件/系统/任务弹窗，直接打开对应邮箱、来源应用或任务详情；自定义邮箱及来源失败回退到记录。点击计时横幅正文打开计时区，其操作按钮仍保留原动作。 | 26 项 Chromium 模拟覆盖各类弹窗。QQ 真邮件弹窗 → 网页由用户本轮确认成功。其余来源的 Windows 桌面点击仍待验证。 |
| Windows 专注助手入口 | 设置 → 通知 → Windows 专注助手 → 打开设置，确认 Windows 设置显示。 | Chromium 模拟已验证按钮调用和说明。Windows 设置实际打开待验证。 |

## 构建复现

在项目根目录安装现有 npm 和 Rust 工具链依赖。Windows 目标在本次验证中由 WSL 构建，使用 WSL Rust、LLVM 链接器和本机 MSVC/Windows SDK 库。运行以下命令时使用配置好的 Windows MSVC 目标工具链。

```sh
npm run vite:build
cargo check --locked --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc
npx vue-tsc --noEmit
npx tauri build --no-bundle --target x86_64-pc-windows-msvc
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib email_imap::tests
```

在 Windows 原生 shell 复现时，最后一条命令会直接执行 Windows 测试。在 WSL 中要添加 `--target x86_64-pc-windows-msvc --no-run`，再到 Windows 执行产出的测试 exe。Windows CI 使用原生命令并且无需邮箱凭据。`vue-tsc` 有四条历史错误，分别是 `useTimer.ts` 的两处算术类型错误和 `PanelApp.vue` 的两处未使用声明，本轮复核仍为这四条。既有 `imap-proto 0.10.2` 在当前工具链可构建，Cargo 会报告未来兼容性警告。

最新 Windows x64 release 于 22:15:33 构建完成，22:17:48 通过桌面「专注岛」快捷方式启动。SHA-256 为 `3ef12ceb36ab96face5f08963cce08012442b3a98533591e04af45af31a17c03`。

Tauri 带显式目标的输出默认在 `src-tauri/target/x86_64-pc-windows-msvc/release/focus-island.exe`，也可能由 `CARGO_TARGET_DIR` 重定向。关闭运行中的 FlowIsle 后，把新产物放到 `src-tauri/target/release/focus-island.exe`。用桌面「专注岛」快捷方式启动，再核对 Windows 进程路径、启动时间和目标文件的 SHA-256。


## 22 点追加验收：QQ、弹窗点击、适中疏密

本轮文件：`src/components/Island.vue`、`SystemPeekBanner.vue`、`NotificationBanner.vue`、`src/panel/PanelApp.vue`、`NotificationsPage.vue`、`src/styles.css`，以及本文档。没有新增项目依赖。TODO 与备忘沿用既有主题，仅调整卡片和列表间距、对齐与邮箱区域字重。

证据目录（Windows）：`C:\Users\lqy\.codex\visualizations\2026\11\13\01a1d3c5-b1eb-7ea0-85e7-73fcb0191619\flowisle-click-update`。目录中的 README 标明模拟、原生和用户反馈的区别。

| Provider / 类型 | 测试项 | 结果 | 可复核证据 |
| --- | --- | --- | --- |
| 163 | 此前真实账号测试 | 用户确认通过；未推定每个负例都已覆盖 | 本轮用户原话“首先前面163的测试是通过的” |
| QQ | 设置页测试连接、保存授权码、开启提醒 | 用户反馈成功；发现保存状态实际为 custom/关闭后，按用户“你自己弄”指示，用本机已有 QQ 凭据恢复为 QQ/开启 | 对话反馈；`enable-qq.log`，22:16:51 |
| QQ | 真实凭据登录、只读打开收件箱、查询未读、能力识别 | 通过，支持 IDLE；原生探测直接复用应用 IMAP 代码与 Windows 凭据读取函数 | `qq-real.log`，22:15:49；不包含凭据或邮件正文 |
| QQ | 重建后重启继续监听 | 通过，快捷方式启动新 exe 后记录 `qq listening IDLE` | `windows-shortcut.log`、`qq-listener.log`，22:17:48/50 |
| QQ | 新邮件 → 岛内弹窗 → 点击 → QQ 邮箱网页 | 用户真实桌面验收通过 | 本轮用户反馈“收到弹窗，点击后打开 QQ 邮箱网页”；`qq-listener.log` 的 22:19:03 事件与 `qq-record-check.log` 的单条记录一致 |
| 五种内置邮箱 | 弹窗直接打开固定收件箱网址 | 页面模拟通过，每次仅打开一次 | `browser.log`、`notification-webview-simulation.mjs` |
| 系统 / 任务 / 计时 | 弹窗直接打开来源应用、任务详情、计时区；失效来源/删除任务回退 | 页面模拟通过；系统通知仅激活来源应用，未获取 Windows 原 Toast 的私有深链参数 | 同上；其他来源真实桌面点击未验证 |
| custom | 无法推导网页地址时定位原记录 | 页面模拟通过 | 同上 |
| TODO / 备忘 / 邮箱设置 | 适中疏密、375/780/1280 三种宽度、邮箱普通字重 | 页面模拟截图已检查；未代替用户视觉接受 | `TODO-*.png`、`备忘-*.png`、`email-weights.png` |
| 构建 | vite:build、cargo check、tauri build --no-bundle | 通过 | `vite-build.log`、`cargo-check.log`、`tauri-build.log` |
| 类型检查 | vue-tsc | 仍为原四条历史错误，无新增 | `vue-tsc.log` |

仍未完成：Gmail 真实授权码全链路；QQ 的错误/过期码、清除凭据、实际 IDLE 降级、真实中文 MIME 与长时间去重、专注静默的完整手动矩阵；其他通知来源的原生桌面跳转。真实 QQ 弹窗成功不等于这些条目全部通过。Windows 界面自动化工具本轮仍因 `sandboxCwd is not a local file URI` 无法初始化，因此桌面点击证据明确归为用户反馈。
