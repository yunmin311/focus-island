# FlowIsle

**屏幕顶部的灵动岛番茄钟，Windows 增强版。**

FlowIsle 是一个 Windows 优先的专注计时器。它以灵动岛/刘海形态常驻屏幕顶部，把当前任务、专注进度、提醒和通知收在一个不打断工作的地方。

> A Windows-focused focus timer built on [FocusIsland](https://github.com/ipfred/focus-island) (Apache-2.0).

---

## 功能

### 灵动岛

- 常驻、停靠、隐藏三种显示方式。
- 默认收为屏幕顶部刘海，悬停展开，点击打开面板。
- 面板打开时保持点击穿透，不遮挡桌面操作。
- 岛与面板透明度独立调节，失焦自动变透明。

### 专注与任务

- 番茄计时、专注/休息自动切换、暂停与提前完成。
- 核心专注区只保留当前最重要的任务。
- 每日任务、重复任务、子任务、到期日、分组。
- 快捷添加任务，支持 `Shift+Enter` 直接开始专注。
- 统计回滚：取消完成时同步回退当日专注数据。
- 成就、连续天数、专注记录与趋势。

### 通知中心

- Windows 系统通知镜像，支持来源筛选与屏蔽。
- 任务提醒、专注完成、邮件提醒统一进入通知中心。
- 岛内 peek 提示，支持悬停暂停、5 秒自动收起、专注时静默。
- 通知按系统/邮件/专注提醒分组，支持单条展开、复制与已读。

### 邮件提醒

- IMAP 新邮件提醒，支持 QQ、163、126、Gmail、Outlook 和自定义 IMAP。
- 163/126 等无 IDLE 服务使用 10 秒轮询；支持 IDLE 的服务使用实时监听。
- 只读取发件人、主题、时间等必要元数据，不读取正文。
- 授权码保存在 Windows 凭据管理器，不写入普通配置文件。

### 其他

- 备忘录、分类、搜索与富文本编辑。
- 专注电台、M3U/PLS 导入、本地音频。
- 系统音量快捷控制。
- 全局快捷键。
- 深色、浅色、跟随系统，五套主题色。
- 自定义空闲标语。

---

## 下载

Windows 安装包发布在 [FlowIsle Releases](https://github.com/yunmin311/flow-isle/releases)。

如果当前版本还没有提供安装包，请按下方源码构建。

> 不要使用应用内更新。FlowIsle 的更新由本仓库发布，应用内更新可能覆盖为上游版本。

---

## 构建

环境要求：

- Windows 10/11
- Node.js 18+
- Rust 1.80+
- Visual Studio C++ Build Tools
- WebView2 Runtime

```powershell
npm install
npx tauri build --no-bundle
```

产物：

```text
src-tauri/target/release/focus-island.exe
```

开发模式：

```powershell
npm run dev
```

---

## 数据与隐私

- 设置、任务、备忘录、通知记录保存在本地 AppData。
- 默认数据目录：`%APPDATA%\com.focus-island.desktop\focus-island`
- 邮箱授权码保存在 Windows 凭据管理器，target 为 `FocusIsland/Email`。
- 系统通知镜像只读取本机通知的应用名、标题、正文与时间，用于本地展示。
- 邮件只读取发件人、主题、时间，不读取正文，不上传服务器。
- 除用户主动配置的 IMAP 邮箱外，不连接外部账号服务。

---

## 上游同步

FlowIsle 基于 [ipfred/focus-island](https://github.com/ipfred/focus-island) v2.9.0，并在此基础上做 Windows 增强。

```powershell
git fetch upstream
git merge upstream/master
```

上游的跨平台发布说明、macOS/Linux 安装步骤和上游 Releases 不适用于本仓库，已从本 README 移除；需要查阅时请直接访问上游仓库。

---

## 许可证与归属

- 本项目基于 FocusIsland，原项目版权归 FocusIsland contributors 所有。
- 上游以 Apache License 2.0 授权；本仓库保留 [LICENSE](LICENSE) 与 [NOTICE](NOTICE)。
- 本仓库对原代码进行了修改，修改部分同样按 Apache License 2.0 分发。
- `FocusIsland` / `专注岛` 的名称、Logo 与上游品牌素材归上游所有；FlowIsle 是独立衍生产品，与上游没有官方关联。
- 第三方依赖和素材的许可证见 [NOTICE](NOTICE) 及各依赖仓库。

---

## 状态

当前为 Windows 开发版。macOS/Linux 适配不在本仓库范围内。
