#[cfg(target_os = "windows")]
mod email_imap;

use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    window::Color,
    AppHandle, Emitter, Manager, Runtime, WebviewWindow, WindowEvent,
};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[cfg(target_os = "macos")]
use core_graphics::event::CGEvent;
#[cfg(target_os = "macos")]
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::POINT;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GetCursorPos, ASFW_ANY,
};

#[cfg(target_os = "linux")]
use x11::xlib;

// GUI 子进程没有可靠的标准输出句柄；app_log! 在写失败时会 panic，
// 后台线程（尤其是邮箱重试）不能因此把整个应用 abort 掉。
macro_rules! app_log {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

const PANEL_GAP_Y: i32 = 28;
const TRAY_ICON_BYTES: &[u8] = include_bytes!("../icons/icon.png");

static LAST_PANEL_SIZE: OnceLock<Mutex<Option<tauri::PhysicalSize<u32>>>> = OnceLock::new();

#[tauri::command]
fn get_local_audio_path<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    // 1) 用户可写目录（推荐）：%APPDATA%\<identifier>\focus-island\music\focus-music.<ext>
    //    应用是用 --no-bundle 构建的，资源目录不会拷贝 assets，所以优先读这里
    // 2) 兼容旧路径：资源目录 assets/audio/focus-music.mp3
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(dir) = app.path().app_data_dir() {
        dirs.push(dir.join("focus-island").join("music"));
    }
    if let Ok(dir) = app.path().resource_dir() {
        dirs.push(dir.join("assets").join("audio"));
    }

    for dir in &dirs {
        for ext in ["mp3", "m4a", "wav", "flac", "ogg"] {
            let candidate = dir.join(format!("focus-music.{ext}"));
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
    }

    Err(format!(
        "本地音乐不存在，把 focus-music.mp3 放到 {}",
        dirs.first()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default()
    ))
}

#[tauri::command]
fn get_update_proxy() -> Option<String> {
    // 1. 优先从环境变量读取（支持终端启动场景）
    let env_keys = [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ];

    for key in env_keys {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("http://")
                || trimmed.starts_with("https://")
                || trimmed.starts_with("socks5://")
            {
                return Some(trimmed.to_string());
            }
            return Some(format!("http://{trimmed}"));
        }
    }

    // 2. 如果环境变量没有，尝试读取系统代理配置（支持 Finder 启动场景）
    #[cfg(target_os = "macos")]
    {
        if let Some(proxy) = get_macos_system_proxy() {
            return Some(proxy);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(proxy) = get_windows_system_proxy() {
            return Some(proxy);
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn get_macos_system_proxy() -> Option<String> {
    // 使用 scutil 命令读取系统代理配置
    let output = std::process::Command::new("scutil")
        .arg("--proxy")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // 解析输出，查找 HTTPSEnable, HTTPSProxy, HTTPSPort
    let mut https_enabled = false;
    let mut https_proxy = String::new();
    let mut https_port = String::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.starts_with("HTTPSEnable") && line.contains(": 1") {
            https_enabled = true;
        } else if line.starts_with("HTTPSProxy") && line.contains(':') {
            if let Some(value) = line.split(':').nth(1) {
                https_proxy = value.trim().to_string();
            }
        } else if line.starts_with("HTTPSPort") && line.contains(':') {
            if let Some(value) = line.split(':').nth(1) {
                https_port = value.trim().to_string();
            }
        }
    }

    if https_enabled && !https_proxy.is_empty() {
        if !https_port.is_empty() {
            return Some(format!("http://{}:{}", https_proxy, https_port));
        } else {
            return Some(format!("http://{}:80", https_proxy));
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn get_windows_system_proxy() -> Option<String> {
    use windows::Win32::Networking::WinInet::{
        InternetQueryOptionA, INTERNET_OPTION_PROXY, INTERNET_PROXY_INFO,
    };

    #[repr(C)]
    struct ProxyQueryBuffer {
        info: INTERNET_PROXY_INFO,
        tail: [u8; 512],
    }

    unsafe {
        let mut buf = ProxyQueryBuffer {
            info: std::mem::zeroed(),
            tail: [0u8; 512],
        };
        let mut size = std::mem::size_of::<ProxyQueryBuffer>() as u32;

        if InternetQueryOptionA(
            None,
            INTERNET_OPTION_PROXY,
            Some(&mut buf.info as *mut INTERNET_PROXY_INFO as *mut _),
            &mut size,
        )
        .is_err()
        {
            return None;
        }

        if buf.info.lpszProxy.is_null() {
            return None;
        }

        let proxy_str = std::ffi::CStr::from_ptr(buf.info.lpszProxy as *const std::ffi::c_char)
            .to_string_lossy()
            .to_string();

        parse_windows_proxy_string(&proxy_str)
    }
}

#[cfg(target_os = "windows")]
fn parse_windows_proxy_string(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("direct") || raw == "direct://" {
        return None;
    }

    // Already a full URL.
    if raw.starts_with("http://")
        || raw.starts_with("https://")
        || raw.starts_with("socks5://")
    {
        return Some(raw.to_string());
    }

    // Per-protocol form: `http=...;https=...;socks=...`
    if raw.contains('=') {
        let mut http_proxy: Option<&str> = None;
        let mut https_proxy: Option<&str> = None;
        let mut socks_proxy: Option<&str> = None;
        for part in raw.split(';') {
            let part = part.trim();
            let Some((scheme, addr)) = part.split_once('=') else {
                continue;
            };
            let addr = addr.trim();
            if addr.is_empty() {
                continue;
            }
            match scheme.trim().to_ascii_lowercase().as_str() {
                "https" => https_proxy = Some(addr),
                "http" => http_proxy = Some(addr),
                "socks" | "socks5" => socks_proxy = Some(addr),
                _ => {}
            }
        }
        // The updater downloads over HTTPS, so prefer the https entry; an http
        // proxy works too because reqwest tunnels https through it via CONNECT.
        if let Some(addr) = https_proxy.or(http_proxy) {
            return Some(format!("http://{addr}"));
        }
        if let Some(addr) = socks_proxy {
            return Some(format!("socks5://{addr}"));
        }
        return None;
    }

    // Bare `host:port` — applies to all protocols.
    Some(format!("http://{raw}"))
}

#[derive(serde::Serialize, Clone, Copy)]
struct PanelTransitionMetrics {
    island_x: i32,
    island_y: i32,
    island_width: u32,
    island_height: u32,
    panel_x: i32,
    panel_y: i32,
    panel_width: u32,
    panel_height: u32,
}

#[derive(serde::Serialize)]
struct MacosUpdateHealth {
    app_path: String,
    quarantined: bool,
    repair_command: String,
}

fn mouse_position_raw() -> (f64, f64) {
    #[cfg(target_os = "macos")]
    if let Ok(source) = CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
        if let Ok(event) = CGEvent::new(source) {
            let point = event.location();
            return (point.x, point.y);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let mut point = POINT::default();
        if unsafe { GetCursorPos(&mut point) }.is_ok() {
            return (point.x as f64, point.y as f64);
        }
    }

    #[cfg(target_os = "linux")]
    unsafe {
        let display = xlib::XOpenDisplay(std::ptr::null());
        if !display.is_null() {
            let mut root_return = 0;
            let mut child_return = 0;
            let mut root_x = 0;
            let mut root_y = 0;
            let mut win_x = 0;
            let mut win_y = 0;
            let mut mask = 0;
            let root = xlib::XDefaultRootWindow(display);
            xlib::XQueryPointer(
                display,
                root,
                &mut root_return,
                &mut child_return,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            );
            xlib::XCloseDisplay(display);
            return (root_x as f64, root_y as f64);
        }
    }

    (0.0, 0.0)
}

// ==== 快捷设置：系统音量/静音 ====

#[derive(serde::Serialize)]
struct VolumeState {
    volume: f32,
    muted: bool,
}

#[cfg(target_os = "windows")]
fn endpoint_volume() -> windows::core::Result<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        device.Activate(CLSCTX_ALL, None)
    }
}

#[tauri::command]
fn get_volume_state() -> Result<VolumeState, String> {
    #[cfg(target_os = "windows")]
    {
        let ev = endpoint_volume().map_err(|e| e.to_string())?;
        unsafe {
            Ok(VolumeState {
                volume: ev.GetMasterVolumeLevelScalar().map_err(|e| e.to_string())?,
                muted: ev.GetMute().map_err(|e| e.to_string())?.as_bool(),
            })
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("仅支持 Windows".into())
    }
}

#[tauri::command]
fn set_system_volume(volume: f32) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let ev = endpoint_volume().map_err(|e| e.to_string())?;
        unsafe {
            ev.SetMasterVolumeLevelScalar(volume.clamp(0.0, 1.0), std::ptr::null())
                .map_err(|e| e.to_string())
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = volume;
        Err("仅支持 Windows".into())
    }
}

#[tauri::command]
fn set_system_mute(muted: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let ev = endpoint_volume().map_err(|e| e.to_string())?;
        unsafe { ev.SetMute(muted, std::ptr::null()).map_err(|e| e.to_string()) }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = muted;
        Err("仅支持 Windows".into())
    }
}

// ==== 全局快捷键：Rust 端注册 ====
// 隐藏窗口里的 JS 注册会被 WebView2 节流/挂起，导致"唤醒经常失败"，
// 因此注册和响应都放在 app 层；设置变更通过 settings-changed 事件热更新。

fn read_shortcut_key(app: &AppHandle) -> String {
    app.path()
        .app_data_dir()
        .ok()
        .and_then(|dir| std::fs::read_to_string(dir.join("focus-island").join("settings.json")).ok())
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("shortcutKey").and_then(|k| k.as_str()).map(|s| s.trim().to_string()))
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| "Alt+Space".to_string())
}

fn apply_global_shortcut(app: &AppHandle, key: &str) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if key.trim().is_empty() {
        app_log!("[shortcut] cleared");
        return;
    }
    let parsed = key.trim().parse::<Shortcut>();
    let Ok(sc) = parsed else {
        app_log!("[shortcut] invalid key: {key}");
        return;
    };
    let result = gs.on_shortcut(sc, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            toggle_panel(app.clone());
        }
    });
    match result {
        Ok(_) => app_log!("[shortcut] registered: {key}"),
        Err(e) => app_log!("[shortcut] register failed: {key} ({e})"),
    }
}

#[tauri::command]
fn set_click_through<R: Runtime>(window: WebviewWindow<R>, ignore: bool) {
    let _ = window.set_ignore_cursor_events(ignore);
}

// ==== 电台导入：视频/歌单链接 → 纯音频 ====

#[derive(serde::Serialize)]
struct AudioResolution {
    stream_url: String,
    title: Option<String>,
}

#[derive(serde::Serialize)]
struct MediaEntry {
    title: String,
    url: String,
}

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// yt-dlp 可执行文件查找顺序：AppData\bin → exe 旁边 → PATH。
#[cfg(target_os = "windows")]
fn ytdlp_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    use std::os::windows::process::CommandExt;
    if let Ok(dir) = app.path().app_data_dir() {
        let p = dir.join("bin").join("yt-dlp.exe");
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.join("yt-dlp.exe");
            if p.exists() {
                return Some(p);
            }
        }
    }
    let out = std::process::Command::new("where")
        .arg("yt-dlp")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let first = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
    let p = std::path::PathBuf::from(first);
    if p.exists() { Some(p) } else { None }
}

/// 确保本机有 yt-dlp；没有则从 GitHub 下载到 AppData\bin（约 17MB，走系统代理）。
#[tauri::command]
fn ensure_ytdlp(app: AppHandle) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        if let Some(p) = ytdlp_path(&app) {
            return Ok(p.to_string_lossy().into_owned());
        }
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("bin");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let target = dir.join("yt-dlp.exe");
        let ps = format!(
            "Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe' -OutFile '{}' -TimeoutSec 300",
            target.to_string_lossy().replace('\'', "''")
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() && target.exists() {
            Ok(target.to_string_lossy().into_owned())
        } else {
            Err("下载 yt-dlp 失败，请检查网络/代理后重试".into())
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("当前平台请自行安装 yt-dlp".into())
    }
}

fn is_direct_audio_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or("");
    [".mp3", ".m4a", ".aac", ".ogg", ".opus", ".wav", ".flac"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

/// 把任意链接解析成纯音频直链：直链直接放行，其余交给 yt-dlp 取 bestaudio。
#[tauri::command]
fn resolve_audio_url(app: AppHandle, url: String) -> Result<AudioResolution, String> {
    if is_direct_audio_url(&url) {
        return Ok(AudioResolution { stream_url: url, title: None });
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let yt = ytdlp_path(&app).ok_or_else(|| "YT_NOT_INSTALLED".to_string())?;
        let out = std::process::Command::new(&yt)
            .args([
                "--no-playlist",
                "-f",
                "bestaudio",
                "--no-warnings",
                "--encoding",
                "utf-8",
                "--socket-timeout",
                "30",
                "--retries",
                "3",
                "--print",
                "%(title)s",
                "--print",
                "%(url)s",
                &url,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr);
            let last = msg.lines().last().unwrap_or("未知错误");
            return Err(format!("解析失败: {last}"));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let title = lines.next().unwrap_or("").trim().to_string();
        let stream = lines.next().unwrap_or("").trim().to_string();
        if stream.is_empty() {
            return Err("未取到音频流".into());
        }
        Ok(AudioResolution {
            stream_url: stream,
            title: if title.is_empty() { None } else { Some(title) },
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("当前平台请先安装 yt-dlp".into())
    }
}

/// 批量导入：视频/合集/歌单链接 → 条目列表（单视频返回 1 条）。
#[tauri::command]
fn import_media_entries(app: AppHandle, url: String) -> Result<Vec<MediaEntry>, String> {
    if is_direct_audio_url(&url) {
        let title = url.rsplit('/').next().unwrap_or("音频直链").to_string();
        return Ok(vec![MediaEntry { title, url }]);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let yt = ytdlp_path(&app).ok_or_else(|| "YT_NOT_INSTALLED".to_string())?;
        let out = std::process::Command::new(&yt)
            .args([
                "--flat-playlist",
                "--no-warnings",
                "--encoding",
                "utf-8",
                "--socket-timeout",
                "30",
                "--retries",
                "3",
                "--print",
                "%(title)s|||%(url)s",
                &url,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr);
            let last = msg.lines().last().unwrap_or("未知错误");
            return Err(format!("解析失败: {last}"));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let entries: Vec<MediaEntry> = text
            .lines()
            .filter(|l| l.contains("|||"))
            .map(|l| {
                let (title, entry_url) = l.split_once("|||").unwrap_or((l, ""));
                let mut u = entry_url.trim().to_string();
                // flat-playlist 模式下单视频的 url 字段可能是 NA：回退到原始链接
                if u.is_empty() || u == "NA" {
                    u = url.clone();
                }
                MediaEntry {
                    title: title.trim().to_string(),
                    url: u,
                }
            })
            .filter(|e| !e.title.is_empty())
            .collect();
        if entries.is_empty() {
            return Err("未解析到任何条目".into());
        }
        Ok(entries)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("当前平台请先安装 yt-dlp".into())
    }
}

/// 拉取远程 M3U/文本歌单（WebView 内受 CORS 限制，走系统 PowerShell）。
#[tauri::command]
fn http_get_text(url: String) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let ps = format!(
            "(Invoke-WebRequest -UseBasicParsing -Uri '{}' -TimeoutSec 20).Content",
            url.replace('\'', "''")
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let out = std::process::Command::new("curl")
            .args(["-sL", "--max-time", "20", &url])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err("fetch failed".into())
        }
    }
}

// ==== Windows 玻璃 v2：材质交给前端 CSS + 抓屏底图 ====
// 教训：浅色主题下系统磨砂是白底，应用必须自己叠足够高的深色 alpha 才是深色玻璃；
// 不要用 SetWindowRgn（会破坏 WebView2 合成），圆角完全交给 CSS 卡片。

// ==== 舞台窗口：灵动岛在固定大小的透明舞台内做纯 CSS 形变 ====

#[derive(Clone, Copy, serde::Serialize)]
struct IslandLayout {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

static ISLAND_LAYOUT: std::sync::Mutex<Option<IslandLayout>> = std::sync::Mutex::new(None);
static PEEK_LAYOUT: std::sync::Mutex<Option<IslandLayout>> = std::sync::Mutex::new(None);
static ISLAND_CLICK_CAPTURE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn island_layout() -> Option<IslandLayout> {
    ISLAND_LAYOUT.lock().ok().and_then(|slot| *slot)
}

fn peek_layout() -> Option<IslandLayout> {
    PEEK_LAYOUT.lock().ok().and_then(|slot| *slot)
}

static LAST_CURSOR: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

/// 系统级用户活动检测：光标自上次查询以来是否移动过。
/// 灵动岛窗口是点击穿透的，收不到真实输入事件，空闲检测必须由 Rust 代劳。
#[tauri::command]
fn cursor_active() -> bool {
    let (mx, my) = mouse_position_raw();
    let (mx, my) = (mx as i32, my as i32);
    let mut slot = LAST_CURSOR.lock().unwrap_or_else(|e| e.into_inner());
    let moved = match *slot {
        Some((lx, ly)) => (lx - mx).abs() + (ly - my).abs() > 2,
        None => true,
    };
    *slot = Some((mx, my));
    moved
}

/// 舞台内的灵动岛把自身屏幕矩形（视口逻辑坐标）上报给 Rust，
/// 面板定位、悬停检测、点击判定全部依赖这个矩形。
#[tauri::command]
fn set_island_layout<R: Runtime>(window: WebviewWindow<R>, x: f64, y: f64, w: f64, h: f64) {
    let Ok(pos) = window.outer_position() else { return };
    let Ok(scale) = window.scale_factor() else { return };
    let layout = IslandLayout {
        x: pos.x + (x * scale).round() as i32,
        y: pos.y + (y * scale).round() as i32,
        w: (w * scale).round() as i32,
        h: (h * scale).round() as i32,
    };
    if let Ok(mut slot) = ISLAND_LAYOUT.lock() {
        *slot = Some(layout);
    }
}

/// 通知横幅的屏幕矩形；只在悬停横幅时让舞台接收鼠标，避免整块 500×160 挡住桌面。
#[tauri::command]
fn set_peek_layout<R: Runtime>(window: WebviewWindow<R>, x: f64, y: f64, w: f64, h: f64) {
    let Ok(pos) = window.outer_position() else { return };
    let Ok(scale) = window.scale_factor() else { return };
    let layout = IslandLayout {
        x: pos.x + (x * scale).round() as i32,
        y: pos.y + (y * scale).round() as i32,
        w: (w * scale).round() as i32,
        h: (h * scale).round() as i32,
    };
    if let Ok(mut slot) = PEEK_LAYOUT.lock() {
        *slot = Some(layout);
    }
}

#[tauri::command]
fn clear_peek_layout() {
    if let Ok(mut slot) = PEEK_LAYOUT.lock() {
        *slot = None;
    }
}

/// 灵动岛本身是点击穿透舞台；开启后钩子会吞掉落在岛矩形内的左键，
/// 避免打开面板的同时点到后面的应用。
#[tauri::command]
fn set_island_click_capture(enabled: bool) {
    ISLAND_CLICK_CAPTURE.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

fn is_mouse_over_island_inner<R: Runtime>(window: &WebviewWindow<R>, pad: f64) -> bool {
    let Some(layout) = island_layout() else { return false };
    let Ok(scale) = window.scale_factor() else { return false };
    let (mx, my) = mouse_position_raw();
    let pad_px = (pad * scale).round() as f64;
    mx >= (layout.x as f64) - pad_px
        && mx <= (layout.x + layout.w) as f64 + pad_px
        && my >= (layout.y as f64) - pad_px
        && my <= (layout.y + layout.h) as f64 + pad_px
}

fn is_mouse_over_peek_inner() -> bool {
    let Some(layout) = peek_layout() else { return false };
    let (mx, my) = mouse_position_raw();
    mx >= layout.x as f64
        && mx <= (layout.x + layout.w) as f64
        && my >= layout.y as f64
        && my <= (layout.y + layout.h) as f64
}

// 低级鼠标钩子：记录左键按下的真实屏幕坐标。
// 不能用 GetAsyncKeyState 的"按下过"位——它与位置无关，
// 在别处点击后 120ms 内移到岛上会被误判成"点了刘海"。
#[cfg(target_os = "windows")]
mod mouse_hook {
    use std::sync::Mutex;
    use std::time::Instant;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, MSLLHOOKSTRUCT, MSG, SetWindowsHookExW, WH_MOUSE_LL,
        WM_LBUTTONDOWN,
    };

    pub static PENDING_CLICK: Mutex<Option<(i32, i32, Instant)>> = Mutex::new(None);

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && wparam.0 as u32 == WM_LBUTTONDOWN {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            if let Ok(mut slot) = PENDING_CLICK.lock() {
                *slot = Some((info.pt.x, info.pt.y, Instant::now()));
            }

            // 岛本身是点击穿透舞台：点击落在岛矩形内时吞掉这次左键，
            // 避免面板打开的同时触发后面应用的按钮。
            let capture = super::ISLAND_CLICK_CAPTURE
                .load(std::sync::atomic::Ordering::Relaxed);
            if capture {
                if let Some(layout) = super::island_layout() {
                    if info.pt.x >= layout.x
                        && info.pt.x <= layout.x + layout.w
                        && info.pt.y >= layout.y
                        && info.pt.y <= layout.y + layout.h
                    {
                        return LRESULT(1);
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    pub fn install() {
        std::thread::spawn(|| unsafe {
            let Ok(_hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), None, 0) else {
                return;
            };
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
        });
    }
}

#[cfg(target_os = "windows")]
fn take_pending_click_inside<R: Runtime>(window: &WebviewWindow<R>, pad: f64) -> bool {
    let Some(layout) = island_layout() else { return false };
    let scale = window.scale_factor().unwrap_or(1.0);
    let pad_px = (pad * scale).round() as i32;

    let Ok(mut slot) = mouse_hook::PENDING_CLICK.lock() else {
        return false;
    };
    let Some((x, y, at)) = slot.take() else {
        return false;
    };
    if at.elapsed().as_millis() > 1200 {
        return false;
    }

    x >= layout.x - pad_px
        && x <= layout.x + layout.w + pad_px
        && y >= layout.y - pad_px
        && y <= layout.y + layout.h + pad_px
}

#[derive(serde::Serialize)]
struct IslandInputState {
    over: bool,
    clicked: bool,
    over_peek: bool,
}

/// 舞台永远点击穿透，输入由该轮询判定。
/// 点击判定使用鼠标钩子记录的坐标，落在岛矩形内才算点击刘海；
/// 其他平台只用悬停判定（点击交给系统原生命中）。
#[tauri::command]
fn poll_island_input<R: Runtime>(window: WebviewWindow<R>, pad: f64) -> IslandInputState {
    let over = is_mouse_over_island_inner(&window, pad);
    let over_peek = is_mouse_over_peek_inner();
    #[cfg(target_os = "windows")]
    {
        let clicked = take_pending_click_inside(&window, pad);
        IslandInputState { over, clicked, over_peek }
    }
    #[cfg(not(target_os = "windows"))]
    {
        IslandInputState { over, clicked: false, over_peek }
    }
}

#[tauri::command]
fn get_window_position<R: Runtime>(window: WebviewWindow<R>) -> (i32, i32) {
    window
        .outer_position()
        .map(|p| (p.x, p.y))
        .unwrap_or((0, 0))
}

#[tauri::command]
fn emit_island_panel_motion<R: Runtime>(app: AppHandle<R>, phase: String) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit("island-panel-motion", phase);
    }
}

fn last_panel_size_lock() -> &'static Mutex<Option<tauri::PhysicalSize<u32>>> {
    LAST_PANEL_SIZE.get_or_init(|| Mutex::new(None))
}

fn set_last_panel_size(size: tauri::PhysicalSize<u32>) {
    if let Ok(mut slot) = last_panel_size_lock().lock() {
        *slot = Some(size);
    }
}

fn get_last_panel_size(fallback: tauri::PhysicalSize<u32>) -> tauri::PhysicalSize<u32> {
    last_panel_size_lock()
        .lock()
        .ok()
        .and_then(|slot| *slot)
        .unwrap_or(fallback)
}

/// B 站等防盗链源：用 yt-dlp 直接下载音频到临时目录，返回本地路径供前端播放。
#[tauri::command]
fn download_temp_audio(app: AppHandle, url: String) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let yt = ytdlp_path(&app).ok_or_else(|| "YT_NOT_INSTALLED".to_string())?;
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("temp-audio");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // 清理上一次的临时音频
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        // 唯一文件名：旧临时音频可能仍被播放器占用（Windows 下删除挂起/写入拒绝），
        // 用固定名会导致切歌或重播时下载失败
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let template = dir.join(format!("audio-{stamp}.%(ext)s"));
        let out = std::process::Command::new(&yt)
            .args([
                "--no-playlist",
                "-f",
                "bestaudio",
                "--no-warnings",
                "--encoding",
                "utf-8",
                "--socket-timeout",
                "30",
                "--retries",
                "3",
                "-o",
                template.to_str().ok_or("无效的临时路径")?,
                &url,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr);
            let last = msg.lines().last().unwrap_or("未知错误");
            return Err(format!("下载失败: {last}"));
        }
        let mut found = None;
        for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let p = e.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase());
            if matches!(ext.as_deref(), Some("m4a") | Some("mp3") | Some("webm") | Some("opus") | Some("ogg") | Some("m4s") | Some("aac")) {
                found = Some(p);
                break;
            }
        }
        found
            .map(|p| p.to_string_lossy().into_owned())
            .ok_or_else(|| "下载完成但未找到音频文件".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        let _ = url;
        Err("仅支持 Windows".into())
    }
}

fn panel_config_values<R: Runtime>(app: &AppHandle<R>) -> (f64, f64, f64, f64) {
    if let Some(cfg) = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "panel")
    {
        let width = cfg.width;
        let height = cfg.height;
        let min_width = cfg.min_width.unwrap_or(width);
        let min_height = cfg.min_height.unwrap_or(height);
        return (width, height, min_width, min_height);
    }
    (420.0, 600.0, 400.0, 480.0)
}

fn panel_target_position(
    island_pos: tauri::PhysicalPosition<i32>,
    island_size: tauri::PhysicalSize<u32>,
    panel_size: tauri::PhysicalSize<u32>,
) -> tauri::PhysicalPosition<i32> {
    let new_x = island_pos.x + ((island_size.width as i32 - panel_size.width as i32) / 2);
    let new_y = island_pos.y + island_size.height as i32 + PANEL_GAP_Y;
    tauri::PhysicalPosition::new(new_x, new_y)
}

/// 灵动岛屏幕矩形：优先用舞台上报告的布局，回退到主窗口矩形。
fn island_rect<R: Runtime>(
    app: &AppHandle<R>,
) -> Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)> {
    if let Some(l) = island_layout() {
        return Some((
            tauri::PhysicalPosition::new(l.x, l.y),
            tauri::PhysicalSize::new(l.w as u32, l.h as u32),
        ));
    }
    let main = app.get_webview_window("main")?;
    Some((main.outer_position().ok()?, main.outer_size().ok()?))
}

fn position_panel_under_island_inner<R: Runtime>(app: &AppHandle<R>) {
    let Some(panel) = app.get_webview_window("panel") else {
        return;
    };
    let Ok(panel_size) = panel.outer_size() else {
        return;
    };
    let Some((main_pos, main_size)) = island_rect(app) else {
        return;
    };
    let target_pos = panel_target_position(main_pos, main_size, panel_size);
    let _ = panel.set_position(target_pos);
}

fn focus_panel<R: Runtime>(app: &AppHandle<R>) {
    // Ensure the main (island) window is visible first (unless user fully hid it)
    if let Some(main) = app.get_webview_window("main") {
        if !main.is_visible().unwrap_or(false) && !island_user_hidden() {
            let _ = main.show();
        }
    }

    let Some(panel) = app.get_webview_window("panel") else {
        // No panel window yet, just focus main
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.set_focus();
        }
        return;
    };

    let _ = panel.unminimize();
    if panel.is_visible().unwrap_or(false) {
        position_panel_under_island_inner(app);
    } else {
        animate_panel_open(app.clone());
    }
    let _ = panel.set_focus();

    // On Windows, background apps cannot steal foreground focus
    // due to OS restrictions. Use AllowSetForegroundWindow workaround.
    #[cfg(target_os = "windows")]
    {
        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
        }
    }
}

#[tauri::command]
fn position_panel_under_island<R: Runtime>(app: AppHandle<R>) {
    position_panel_under_island_inner(&app);
    if let Some(panel) = app.get_webview_window("panel") {
        // 从屏幕外移回后恢复交互
        let _ = panel.set_ignore_cursor_events(false);
        // 强制重绘，避免 DWM 把旧位置的帧留在新位置
        #[cfg(target_os = "windows")]
        force_repaint(&panel);
    }
}

#[tauri::command]
fn get_panel_transition_metrics<R: Runtime>(app: AppHandle<R>) -> Option<PanelTransitionMetrics> {
    let Some(panel) = app.get_webview_window("panel") else {
        return None;
    };
    let Some((main_pos, main_size)) = island_rect(&app) else {
        return None;
    };

    let (cfg_w, cfg_h, cfg_min_w, cfg_min_h) = panel_config_values(&app);
    let scale = panel.scale_factor().unwrap_or(1.0);
    let fallback_size = panel.outer_size().unwrap_or(tauri::PhysicalSize::new(
        (cfg_w * scale).round() as u32,
        (cfg_h * scale).round() as u32,
    ));
    let panel_size = if panel.is_visible().unwrap_or(false) {
        panel.outer_size().unwrap_or(fallback_size)
    } else {
        get_last_panel_size(fallback_size)
    };
    let min_width = (cfg_min_w * scale).round() as u32;
    let min_height = (cfg_min_h * scale).round() as u32;
    let panel_size = tauri::PhysicalSize::new(
        panel_size.width.max(min_width),
        panel_size.height.max(min_height),
    );
    let panel_pos = panel_target_position(main_pos, main_size, panel_size);

    Some(PanelTransitionMetrics {
        island_x: main_pos.x,
        island_y: main_pos.y,
        island_width: main_size.width,
        island_height: main_size.height,
        panel_x: panel_pos.x,
        panel_y: panel_pos.y,
        panel_width: panel_size.width,
        panel_height: panel_size.height,
    })
}

#[tauri::command]
fn force_current_window_frameless(window: tauri::WebviewWindow) {
    #[cfg(target_os = "windows")]
    force_frameless_window(&window);
}

#[tauri::command]
fn show_panel<R: Runtime>(app: AppHandle<R>, focus: Option<bool>) {
    if focus.unwrap_or(false) {
        focus_panel(&app);
    } else {
        animate_panel_open(app);
    }
}

#[cfg(target_os = "windows")]
const NATIVE_FRAME_STYLE: u32 = 0x00C0_0000 | 0x0004_0000 | 0x0008_0000 | 0x0002_0000 | 0x0001_0000;

/// Windows/WebView2 有时会在无边框窗口保留原生标题栏和边框，直接移除对应的 Win32 样式。
#[cfg(target_os = "windows")]
fn force_frameless_window<R: Runtime>(window: &WebviewWindow<R>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    };

    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd = HWND(hwnd.0);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        // WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX
        let mask = NATIVE_FRAME_STYLE as isize;
        // 已经是无边框就什么都不做：每次焦点变化都 SetWindowPos(FRAMECHANGED) 会让 DWM 重算边框，
        // 亮面/亚克力下表现为“点一下闪一下”。
        if (style & mask) == 0 {
            return;
        }
        SetWindowLongPtrW(hwnd, GWL_STYLE, style & !mask);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// 关闭透明窗口的非客户区渲染、系统圆角和边框色。
#[cfg(target_os = "windows")]
fn disable_native_window_frame<R: Runtime>(window: &WebviewWindow<R>, guard_style: bool) {
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_NCRENDERING_POLICY,
        DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_WINDOW_CORNER_PREFERENCE, DWMNCRP_DISABLED,
        DWMWCP_DONOTROUND,
    };
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, STYLESTRUCT, WM_NCACTIVATE, WM_NCDESTROY, WM_NCPAINT, WM_STYLECHANGING,
    };

    unsafe extern "system" fn frameless_window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        subclass_id: usize,
        guard_style: usize,
    ) -> LRESULT {
        if msg == WM_STYLECHANGING && guard_style != 0 && wparam.0 as i32 == GWL_STYLE.0 {
            // 在系统提交样式前移除非客户区标志，无需再次应用样式或重绘。
            let style = unsafe { &mut *(lparam.0 as *mut STYLESTRUCT) };
            style.styleNew &= !NATIVE_FRAME_STYLE;
        }
        if msg == WM_NCPAINT {
            return LRESULT(0);
        }
        let lparam = if msg == WM_NCACTIVATE {
            // 把激活消息交给 Tao 更新焦点状态，同时让 DefWindowProc 跳过标题栏重绘。
            LPARAM(-1)
        } else {
            lparam
        };
        unsafe {
            if msg == WM_NCDESTROY {
                let _ = RemoveWindowSubclass(hwnd, Some(frameless_window_proc), subclass_id);
            }
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
    }

    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd = HWND(hwnd.0);
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(frameless_window_proc), 0, guard_style as usize);
        let policy = DWMNCRP_DISABLED;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            &policy as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&policy) as u32,
        );

        let pref = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&pref) as u32,
        );

        let border = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&border) as u32,
        );

        // 关闭 DWM 过渡动画：透明窗口在获得/失去焦点时，系统过渡会把上一帧再合成一次，
        // 表现为“点一下面板，背后那一层闪一下”。关掉后焦点切换不再有过渡帧。
        let no_transition: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &no_transition as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&no_transition) as u32,
        );
    }
}

/// 透明窗口在尺寸/位置变化后可能留下旧帧，强制重绘一次避免“两层材质/残影”。
#[cfg(target_os = "windows")]
fn force_repaint<R: Runtime>(window: &WebviewWindow<R>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::DwmFlush;
    use windows::Win32::Graphics::Gdi::{
        RedrawWindow, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW,
    };

    let Ok(hwnd) = window.hwnd() else { return };
    unsafe {
        let _ = RedrawWindow(
            Some(HWND(hwnd.0)),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
        );
        let _ = DwmFlush();
    }
}

// 面板圆角完全交给 CSS（.panel-card 的 border-radius + overflow: clip）。
// 不用 SetWindowRgn：DWM 系统材质会忽略窗口区域，区域变更本身也会触发整窗重绘（点击闪烁）。

// 面板打开/关闭的代次，用于关闭看门狗避免误伤新打开的面板
static PANEL_OPEN_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 灵动岛/面板都是伴随窗口，不能出现在任务栏：否则悬停图标会出现“白页 + 标题 + 关闭”的缩略图。
/// Tauri 的 skip_taskbar 只删一次按钮，窗口再次 show 时按钮会回来，所以这里直接改扩展样式。
#[cfg(target_os = "windows")]
fn apply_window_skip_taskbar<R: Runtime>(window: &WebviewWindow<R>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };

    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd = HWND(hwnd.0);
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        // 已经是工具窗口且没有任务栏位时直接返回，避免每次开关面板都触发任务栏/DWM 合成（会闪）
        if (ex & WS_EX_TOOLWINDOW.0 as isize) != 0 && (ex & WS_EX_APPWINDOW.0 as isize) == 0 {
            return;
        }
        let new = (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
        drop_taskbar_button(hwnd);
    }
}

#[cfg(target_os = "windows")]
fn drop_taskbar_button(hwnd: windows::Win32::Foundation::HWND) {
    use windows::core::IUnknown;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{ITaskbarList, TaskbarList};

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let list: windows::core::Result<ITaskbarList> =
            CoCreateInstance(&TaskbarList, None::<&IUnknown>, CLSCTX_INPROC_SERVER);
        if let Ok(list) = list {
            let _ = list.HrInit();
            let _ = list.DeleteTab(hwnd);
        }
    }
}

// 玻璃质感（亮面 / 截图磨砂 / 亚克力）全部由前端用 CSS + 抓屏底图渲染。
// 不再开启 DWM 系统材质：原生亚克力的矩形模糊会无视窗口 region，
// 从 CSS 圆角外露出四个方角；反复切 DWM 属性也会让整窗重绘（点击闪烁）。

#[tauri::command]
fn hide_panel<R: Runtime>(app: AppHandle<R>) {
    if let Some(panel) = app.get_webview_window("panel") {
        if let Ok(size) = panel.outer_size() {
            set_last_panel_size(size);
        }
        // 先把已经全透明的最后一帧刷给 DWM，再隐藏，避免系统保留上一帧页头
        #[cfg(target_os = "windows")]
        force_repaint(&panel);
        let _ = panel.hide();
        // hide 之后系统会重新挂上任务栏按钮，这里再压一次，保证收起状态也不出现白页缩略图
        let _ = panel.set_skip_taskbar(true);
        #[cfg(target_os = "windows")]
        apply_window_skip_taskbar(&panel);
    }
    // 兜底：无论面板从哪条路径关闭，都让灵动岛恢复刘海态
    let _ = app.emit("island-panel-motion", "close");
}

#[tauri::command]
fn toggle_panel<R: Runtime>(app: AppHandle<R>) {
    if let Some(panel) = app.get_webview_window("panel") {
        if panel.is_visible().unwrap_or(false) {
            animate_panel_close(app.clone());
        } else {
            animate_panel_open(app.clone());
        }
    }
}

#[tauri::command]
fn animate_panel_open<R: Runtime>(app: AppHandle<R>) {
    let Some(panel) = app.get_webview_window("panel") else {
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        return;
    }
    // 新的一次打开：让上一轮的关闭看门狗失效
    PANEL_OPEN_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let Some((main_pos, main_size)) = island_rect(&app) else {
        return;
    };

    let (cfg_w, cfg_h, cfg_min_w, cfg_min_h) = panel_config_values(&app);
    let scale = panel.scale_factor().unwrap_or(1.0);
    // 固定尺寸：面板不做用户缩放，避免窗口尺寸漂移后内容与玻璃材质错位（会看到两层边缘）
    let target_size = tauri::PhysicalSize::new(
        (cfg_w * scale).round() as u32,
        (cfg_h * scale).round() as u32,
    );
    let min_width = (cfg_min_w * scale).round() as u32;
    let min_height = (cfg_min_h * scale).round() as u32;
    let target_size = tauri::PhysicalSize::new(
        target_size.width.max(min_width),
        target_size.height.max(min_height),
    );
    let target_pos = panel_target_position(main_pos, main_size, target_size);

    let _ = panel.set_ignore_cursor_events(true);
    // 先放到屏幕外显示，由前端准备好打开动画后移回目标位置，避免旧帧或白底露出。
    let offscreen_pos = tauri::PhysicalPosition::new(target_pos.x, -(target_size.height as i32) - 200);
    let _ = panel.set_position(offscreen_pos);
    let _ = panel.set_size(target_size);
    let _ = panel.set_min_size(Some(tauri::LogicalSize::new(cfg_min_w, cfg_min_h)));

    if let Some(main) = app.get_webview_window("main") {
        if !main.is_visible().unwrap_or(true) && !island_user_hidden() {
            let _ = main.show();
            let _ = main.set_skip_taskbar(true);
            #[cfg(target_os = "windows")]
            apply_window_skip_taskbar(&main);
        }
    }
    let _ = panel.set_decorations(false);
    #[cfg(target_os = "windows")]
    force_frameless_window(&panel);
    #[cfg(target_os = "windows")]
    apply_window_skip_taskbar(&panel);
    let _ = panel.show();
    #[cfg(target_os = "windows")]
    apply_window_skip_taskbar(&panel);
    #[cfg(target_os = "windows")]
    force_frameless_window(&panel);
    #[cfg(target_os = "windows")]
    force_repaint(&panel);
    let _ = panel.emit("panel-window-transition", "open");
    // 激活/聚焦过程中 Tauri 或系统会再次挂上 WS_CAPTION（原生标题栏“专注清单”），
    // 它平时不画，但焦点变化时会闪出来。打开后 2.5 秒内持续把 caption 剥掉。
    #[cfg(target_os = "windows")]
    {
        let guard = app.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(125));
                let Some(panel) = guard.get_webview_window("panel") else { break };
                if !panel.is_visible().unwrap_or(false) {
                    break;
                }
                force_frameless_window(&panel);
            }
        });
    }
}

#[tauri::command]
fn animate_panel_close<R: Runtime>(app: AppHandle<R>) {
    let Some(panel) = app.get_webview_window("panel") else {
        return;
    };
    if !panel.is_visible().unwrap_or(false) {
        return;
    }

    if let Ok(size) = panel.outer_size() {
        set_last_panel_size(size);
    }
    let generation = PANEL_OPEN_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if panel.emit("panel-window-transition", "close").is_err() {
        let _ = panel.hide();
        let _ = panel.set_skip_taskbar(true);
        #[cfg(target_os = "windows")]
        apply_window_skip_taskbar(&panel);
        return;
    }
    // 看门狗：前端动画结束/定时器偶发丢失时兜底隐藏，避免面板收起后残留窗口边缘
    let app_handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(700));
        if PANEL_OPEN_GEN.load(std::sync::atomic::Ordering::SeqCst) != generation {
            return;
        }
        if let Some(panel) = app_handle.get_webview_window("panel") {
            if panel.is_visible().unwrap_or(false) {
                #[cfg(target_os = "windows")]
                force_repaint(&panel);
                let _ = panel.hide();
                let _ = panel.set_skip_taskbar(true);
                #[cfg(target_os = "windows")]
                apply_window_skip_taskbar(&panel);
                let _ = app_handle.emit("island-panel-motion", "close");
            }
        }
    });
}

#[tauri::command]
fn is_panel_visible<R: Runtime>(app: AppHandle<R>) -> bool {
    if let Some(panel) = app.get_webview_window("panel") {
        return panel.is_visible().unwrap_or(false);
    }
    false
}

static ISLAND_USER_HIDDEN: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

fn island_user_hidden() -> bool {
    ISLAND_USER_HIDDEN.lock().map(|f| *f).unwrap_or(false)
}

#[tauri::command]
fn set_island_visible<R: Runtime>(app: AppHandle<R>, visible: bool) {
    if let Ok(mut f) = ISLAND_USER_HIDDEN.lock() {
        *f = !visible;
    }
    if let Some(main) = app.get_webview_window("main") {
        if visible {
            let _ = main.show();
            let _ = main.set_skip_taskbar(true);
            #[cfg(target_os = "windows")]
            apply_window_skip_taskbar(&main);
        } else {
            let _ = main.hide();
        }
    }
}

#[tauri::command]
fn get_screen_info<R: Runtime>(app: AppHandle<R>) -> (f64, f64, f64) {
    if let Some(main) = app.get_webview_window("main") {
        if let Ok(Some(monitor)) = main.current_monitor() {
            let scale_factor = monitor.scale_factor();
            let physical_width = monitor.size().width as f64;
            let position = monitor.position();
            return (physical_width, scale_factor, position.x as f64);
        }
    }
    (1920.0, 1.0, 0.0)
}

#[tauri::command]
fn get_macos_update_health<R: Runtime>(_app: AppHandle<R>) -> Option<MacosUpdateHealth> {
    #[cfg(target_os = "macos")]
    {
        let executable = std::env::current_exe().ok()?;
        let mut app_bundle = None;
        for parent in executable.ancestors() {
            if parent.extension().and_then(|e| e.to_str()) == Some("app") {
                app_bundle = Some(parent.to_path_buf());
                break;
            }
        }
        let app_bundle = app_bundle?;
        let app_path = app_bundle.to_string_lossy().into_owned();
        let quarantined = std::process::Command::new("xattr")
            .arg("-p")
            .arg("com.apple.quarantine")
            .arg(&app_bundle)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);

        return Some(MacosUpdateHealth {
            repair_command: format!("xattr -dr com.apple.quarantine \"{app_path}\""),
            app_path,
            quarantined,
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

// ==== 系统通知镜像（Windows，MVP） ====
// 免安装 Win32 应用通过 UserNotificationListener 读取通知中心里的 Toast 历史。
// 采用轮询而非事件订阅：启动首轮只做 seed（不回放旧通知），之后每 1.5s
// 读取一次，按 (id, created_ms) 去重，新通知以 "system-notification" 事件发给前端。
// 注意：系统不允许拦截或关闭原生 Toast，这里只是镜像。

#[cfg(target_os = "windows")]
#[derive(Clone, serde::Serialize)]
struct SystemNotificationEvent {
    id: u32,
    app: String,
    aumid: String,
    title: String,
    body: String,
    created_ms: i64,
}

#[cfg(target_os = "windows")]
fn read_system_notification_event(
    n: &windows::UI::Notifications::UserNotification,
) -> SystemNotificationEvent {
    use windows::UI::Notifications::KnownNotificationBindings;

    let mut app = String::new();
    let mut aumid = String::new();
    if let Ok(info) = n.AppInfo() {
        if let Ok(display) = info.DisplayInfo() {
            app = display.DisplayName().map(|s| s.to_string()).unwrap_or_default();
        }
        aumid = info.AppUserModelId().map(|s| s.to_string()).unwrap_or_default();
    }

    let mut title = String::new();
    let mut body = String::new();
    if let Ok(notification) = n.Notification() {
        if let Ok(visual) = notification.Visual() {
            if let Ok(generic) = KnownNotificationBindings::ToastGeneric() {
                if let Ok(binding) = visual.GetBinding(&generic) {
                    if let Ok(texts) = binding.GetTextElements() {
                        if let Ok(t) = texts.GetAt(0) {
                            title = t.Text().map(|s| s.to_string()).unwrap_or_default();
                        }
                        if texts.Size().unwrap_or(0) > 1 {
                            if let Ok(t) = texts.GetAt(1) {
                                body = t.Text().map(|s| s.to_string()).unwrap_or_default();
                            }
                        }
                    }
                }
            }
        }
    }

    let created_ms = n
        .CreationTime()
        .map(|dt| (dt.UniversalTime / 10_000) - 11_644_473_600_000)
        .unwrap_or(0);

    SystemNotificationEvent {
        id: n.Id().unwrap_or(0),
        app,
        aumid,
        title,
        body,
        created_ms,
    }
}

#[cfg(target_os = "windows")]
fn start_system_notification_mirror<R: Runtime>(app: AppHandle<R>) {
    let spawned = std::thread::Builder::new()
        .name("system-notification-mirror".into())
        .spawn(move || {
            use std::collections::{HashSet, VecDeque};
            use std::time::Duration;
            use windows::UI::Notifications::Management::{
                UserNotificationListener, UserNotificationListenerAccessStatus,
            };
            use windows::UI::Notifications::NotificationKinds;

            // WinRT 监听器需要在 MTA 线程上初始化 COM
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_MULTITHREADED,
                );
            }

            let listener = match UserNotificationListener::Current() {
                Ok(listener) => listener,
                Err(e) => {
                    app_log!("[system-notif] listener unavailable: {e}");
                    return;
                }
            };

            match listener.RequestAccessAsync().and_then(|op| op.get()) {
                Ok(UserNotificationListenerAccessStatus::Allowed) => {
                    app_log!("[system-notif] access allowed");
                }
                Ok(status) => {
                    app_log!("[system-notif] access not allowed: {status:?}");
                    return;
                }
                Err(e) => {
                    app_log!("[system-notif] access request failed: {e}");
                    return;
                }
            }

            let mut seen: HashSet<(u32, i64)> = HashSet::new();
            let mut seen_order: VecDeque<(u32, i64)> = VecDeque::new();
            let mut seeded = false;
            let mut last_error: Option<String> = None;

            loop {
                match listener
                    .GetNotificationsAsync(NotificationKinds::Toast)
                    .and_then(|op| op.get())
                {
                    Ok(items) => {
                        last_error = None;
                        let count = items.Size().unwrap_or(0);
                        for i in 0..count {
                            let Ok(n) = items.GetAt(i) else { continue };
                            let ev = read_system_notification_event(&n);
                            let key = (ev.id, ev.created_ms);
                            if seen.contains(&key) {
                                continue;
                            }
                            seen.insert(key);
                            seen_order.push_back(key);
                            while seen_order.len() > 500 {
                                if let Some(old) = seen_order.pop_front() {
                                    seen.remove(&old);
                                }
                            }
                            // 首轮只是记录已有通知，不往前端回放
                            if seeded
                                && !(ev.title.is_empty()
                                    && ev.body.is_empty()
                                    && ev.app.is_empty())
                            {
                                let _ = app.emit("system-notification", ev);
                            }
                        }
                        seeded = true;
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if last_error.as_deref() != Some(msg.as_str()) {
                            app_log!("[system-notif] poll failed: {msg}");
                            last_error = Some(msg);
                        }
                    }
                }
                // 未打包 Win32 应用无法订阅 NotificationChanged，只能轮询；
                // 1s 间隔在 CPU 开销和镜像延迟之间取平衡。
                std::thread::sleep(Duration::from_millis(1000));
            }
        });

    if let Err(e) = spawned {
        app_log!("[system-notif] failed to start mirror thread: {e}");
    }
}

// ==== 邮件通知（Windows）：IMAP 只取发件人/主题/时间 ====
// 凭据存 Windows 凭据管理器；QQ/163 用授权码，Gmail 用应用专用密码。
// 优先 IDLE；服务不支持时每 10 秒轮询。

#[cfg(target_os = "windows")]
const EMAIL_CRED_TARGET: &str = "FocusIsland/Email";

#[cfg(target_os = "windows")]
#[derive(Clone, PartialEq)]
struct EmailConfig {
    enabled: bool,
    provider: String,
    address: String,
    custom_host: String,
}

#[cfg(target_os = "windows")]
#[derive(Clone, serde::Serialize)]
struct EmailNotificationEvent {
    uid: u32,
    from: String,
    subject: String,
    date: String,
    provider: String,
    mailbox_key: String,
}

#[cfg(target_os = "windows")]
fn save_email_credential_blocking(email: &str, password: &str, endpoint: &str) -> Result<(), String> {
    use windows::core::PWSTR;
    use windows::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    let mut target: Vec<u16> = EMAIL_CRED_TARGET
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut user: Vec<u16> = email
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut blob = password.as_bytes().to_vec();
    let mut endpoint: Vec<u16> = endpoint.encode_utf16().chain([0]).collect();

    let credential = CREDENTIALW {
        Flags: Default::default(),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        Comment: PWSTR(endpoint.as_mut_ptr()),
        LastWritten: Default::default(),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR(user.as_mut_ptr()),
    };

    unsafe { CredWriteW(&credential, 0) }.map_err(|_| "保存 Windows 凭据失败".into())
}

#[cfg(target_os = "windows")]
fn read_email_credential_blocking() -> Option<(String, String, String)> {
    use windows::core::PCWSTR;
    use windows::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target: Vec<u16> = EMAIL_CRED_TARGET
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut credential_ptr: *mut CREDENTIALW = std::ptr::null_mut();
    if unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut credential_ptr) }
        .is_err()
        || credential_ptr.is_null()
    {
        return None;
    }

    let credential = unsafe { &*credential_ptr };
    let user = if credential.UserName.is_null() {
        String::new()
    } else {
        unsafe { credential.UserName.to_string() }.unwrap_or_default()
    };
    let password = if credential.CredentialBlob.is_null() || credential.CredentialBlobSize == 0 {
        String::new()
    } else {
        let bytes = unsafe {
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
        };
        String::from_utf8_lossy(bytes).to_string()
    };
    let endpoint = if credential.Comment.is_null() {
        String::new()
    } else {
        unsafe { credential.Comment.to_string() }.unwrap_or_default()
    };
    unsafe { CredFree(credential_ptr as *const _) };
    Some((user, password, endpoint))
}

#[cfg(target_os = "windows")]
fn delete_email_credential_blocking() -> Result<(), String> {
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::core::PCWSTR;
    use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
    let target: Vec<u16> = EMAIL_CRED_TARGET
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(error) if error.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(_) => Err("清除 Windows 凭据失败".into()),
    }
}

#[cfg(target_os = "windows")]
fn read_email_config<R: Runtime>(app: &AppHandle<R>) -> Option<EmailConfig> {
    let dir = app.path().app_data_dir().ok()?.join("focus-island");
    let raw = std::fs::read_to_string(dir.join("settings.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some(EmailConfig {
        enabled: value
            .get("emailEnabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        provider: value
            .get("emailProvider")
            .and_then(|v| v.as_str())
            .unwrap_or("qq")
            .to_string(),
        address: value
            .get("emailAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
        custom_host: value
            .get("emailImapHost")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
    })
}

#[cfg(target_os = "windows")]
fn stored_email_password(provider: &str, address: &str, host: &str) -> Option<String> {
    let (stored_user, password, endpoint) = read_email_credential_blocking()?;
    if !stored_user.eq_ignore_ascii_case(address) || password.is_empty() {
        return None;
    }
    if endpoint == format!("{provider}:{host}") {
        return Some(password);
    }
    // Legacy entries had no endpoint. Reuse only for the provider's own domains
    // and fixed TLS host; a changed custom endpoint always requires a new save.
    let (_, domain) = address.rsplit_once('@')?;
    let legacy_provider = match domain.to_ascii_lowercase().as_str() {
        "qq.com" | "foxmail.com" => "qq",
        "163.com" => "163",
        "126.com" => "126",
        "gmail.com" | "googlemail.com" => "gmail",
        "outlook.com" | "hotmail.com" | "live.com" => "outlook",
        _ => return None,
    };
    (endpoint.is_empty() && provider == legacy_provider).then_some(password)
}

#[cfg(target_os = "windows")]
fn email_diagnostic<R: Runtime>(app: &AppHandle<R>, provider: &str, message: &str) {
    use std::io::Write;
    let Ok(dir) = app.path().app_data_dir() else { return };
    let dir = dir.join("focus-island");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("email-status.log");
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 64 * 1024) {
        let _ = std::fs::remove_file(&path);
    }
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs();
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{time} {provider} {message}");
    }
}

#[cfg(target_os = "windows")]
fn run_email_imap_session<R: Runtime>(
    app: &AppHandle<R>,
    config: &EmailConfig,
    host: &str,
    password: &str,
    cursor: &mut email_imap::MailboxCursor,
) -> Result<(), email_imap::Error> {
    use std::hash::{Hash, Hasher};

    let current = || {
        read_email_config(app).as_ref() == Some(config)
            && stored_email_password(&config.provider, &config.address, host).as_deref() == Some(password)
    };
    let mut session = email_imap::connect_imap(host)?;
    let uid_validity = session.open_inbox(&config.provider, &config.address, password)?;
    let mut supports_idle = session.supports_idle()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (&config.provider, host, config.address.to_ascii_lowercase(), uid_validity).hash(&mut hasher);
    let mailbox_key = format!("{:016x}", hasher.finish());
    email_diagnostic(app, &config.provider, if supports_idle { "listening IDLE" } else { "listening poll" });

    loop {
        if !current() {
            return Ok(());
        }
        let pending = cursor.pending(uid_validity, session.unread()?);
        for chunk in pending.chunks(50) {
            for mail in session.fetch(chunk)? {
                if !current() {
                    return Ok(());
                }
                let event = EmailNotificationEvent {
                    uid: mail.uid,
                    from: mail.from,
                    subject: mail.subject,
                    date: mail.date,
                    provider: config.provider.clone(),
                    mailbox_key: mailbox_key.clone(),
                };
                if app.emit("email-notification", event).is_ok() {
                    cursor.mark_seen(mail.uid);
                    email_diagnostic(app, &config.provider, "notification emitted");
                }
            }
        }
        if supports_idle {
            supports_idle = session.idle()?;
            if !supports_idle {
                email_diagnostic(app, &config.provider, "IDLE rejected; listening poll");
            }
        }
        if !supports_idle {
            std::thread::sleep(Duration::from_secs(10));
        }
    }
}

#[cfg(target_os = "windows")]
fn start_email_watcher<R: Runtime>(app: AppHandle<R>) {
    let spawned = std::thread::Builder::new()
        .name("email-imap".into())
        .spawn(move || {
            let mut cursor = email_imap::MailboxCursor::default();
            let mut active_account = None;
            let mut failures = 0u32;
            loop {
                let Some(config) = read_email_config(&app) else {
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                };
                let host = email_imap::email_imap_host(&config.provider, &config.custom_host);
                let account = (config.provider.clone(), config.address.to_ascii_lowercase(), host.clone());
                if active_account.as_ref() != Some(&account) {
                    cursor = email_imap::MailboxCursor::default();
                    failures = 0;
                    active_account = Some(account);
                }
                let Some(host) = host.filter(|_| config.enabled && !config.address.is_empty()) else {
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                };
                let Some(password) = stored_email_password(&config.provider, &config.address, &host) else {
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                };
                match run_email_imap_session(&app, &config, &host, &password, &mut cursor) {
                    Ok(()) => failures = 0,
                    Err(error) => {
                        failures = failures.saturating_add(1);
                        email_diagnostic(&app, &config.provider, error.message(&config.provider));
                        let delay = match failures { 1 => 15, 2 => 30, 3 => 60, _ => 120 };
                        for _ in 0..delay / 2 {
                            std::thread::sleep(Duration::from_secs(2));
                            if read_email_config(&app).as_ref() != Some(&config)
                                || stored_email_password(&config.provider, &config.address, &host).as_deref() != Some(&password)
                            {
                                failures = 0;
                                break;
                            }
                        }
                    }
                }
            }
        });
    if let Err(e) = spawned {
        app_log!("[email] failed to start watcher: {e}");
    }
}

#[cfg(target_os = "windows")]
fn test_email_connection_blocking(
    provider: &str,
    email: &str,
    custom_host: &str,
    input_password: Option<&str>,
) -> Result<String, String> {
    let email = email.trim();
    if email.is_empty() {
        return Err("请先填写邮箱地址".into());
    }
    let error_message = |error: email_imap::Error| error.message(provider).to_owned();
    let host = email_imap::email_imap_host(provider, custom_host)
        .ok_or_else(|| error_message(email_imap::Error::Host))?;
    let password = input_password.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
        .or_else(|| stored_email_password(provider, email, &host))
        .ok_or("请填写授权码，或为当前邮箱和 IMAP 服务器重新保存授权码")?;
    let mut session = email_imap::connect_imap(&host).map_err(error_message)?;
    session.open_inbox(provider, email, &password).map_err(error_message)?;
    let unseen = session.unread().map_err(error_message)?.len();
    let mode = if session.supports_idle().map_err(error_message)? { "支持 IDLE" } else { "10 秒轮询" };
    Ok(format!("连接成功：收件箱可用，未读 {unseen} 封（{mode}）"))
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn save_email_credential(email: String, password: String, provider: String, host: String) -> Result<(), String> {
    let email = email.trim();
    let password = password.trim();
    if email.is_empty() || password.is_empty() {
        return Err("请填写邮箱地址和授权码".into());
    }
    let host = email_imap::email_imap_host(&provider, &host)
        .ok_or_else(|| email_imap::Error::Host.message(&provider).to_owned())?;
    save_email_credential_blocking(email, password, &format!("{provider}:{host}"))
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn email_credential_status(email: String, provider: String, host: String) -> Result<bool, String> {
    Ok(email_imap::email_imap_host(&provider, &host).is_some_and(|host| {
        stored_email_password(&provider, email.trim(), &host).is_some()
    }))
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn delete_email_credential() -> Result<(), String> {
    delete_email_credential_blocking()
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn test_email_connection(
    provider: String,
    email: String,
    host: String,
    password: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        test_email_connection_blocking(&provider, &email, &host, password.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(target_os = "windows")]
#[derive(serde::Serialize)]
struct NotificationSourceInfo {
    app: String,
    aumid: String,
}

#[cfg(target_os = "windows")]
fn probe_notification_sources_blocking() -> Result<Vec<NotificationSourceInfo>, String> {
    use windows::UI::Notifications::Management::{
        UserNotificationListener, UserNotificationListenerAccessStatus,
    };
    use windows::UI::Notifications::NotificationKinds;

    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }

    let listener = UserNotificationListener::Current().map_err(|e| e.to_string())?;
    let mut status = listener.GetAccessStatus().map_err(|e| e.to_string())?;
    if status != UserNotificationListenerAccessStatus::Allowed {
        status = listener
            .RequestAccessAsync()
            .and_then(|op| op.get())
            .map_err(|e| e.to_string())?;
    }
    if status != UserNotificationListenerAccessStatus::Allowed {
        return Err("没有通知读取权限".into());
    }

    let items = listener
        .GetNotificationsAsync(NotificationKinds::Toast)
        .and_then(|op| op.get())
        .map_err(|e| e.to_string())?;

    let mut seen: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    let mut sources = Vec::new();
    for index in 0..items.Size().unwrap_or(0) {
        let Ok(notification) = items.GetAt(index) else {
            continue;
        };
        let event = read_system_notification_event(&notification);
        if event.aumid.to_lowercase().contains("com.focus-island") {
            continue;
        }
        let app = event.app.trim().to_string();
        if app.is_empty() {
            continue;
        }
        if seen.insert((app.clone(), event.aumid.clone())) {
            sources.push(NotificationSourceInfo {
                app,
                aumid: event.aumid,
            });
        }
    }

    sources.sort_by(|a, b| a.app.to_lowercase().cmp(&b.app.to_lowercase()));
    Ok(sources)
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn probe_notification_sources() -> Result<Vec<NotificationSourceInfo>, String> {
    tauri::async_runtime::spawn_blocking(probe_notification_sources_blocking)
        .await
        .map_err(|e| e.to_string())?
}

/// 按 AUMID 打开通知来源应用。
/// 打包/UWP 应用走 IApplicationActivationManager；Win32 应用回退 AppsFolder。
/// 两条路径都失败时返回明确错误，前端提示“无法定位来源应用”。
#[cfg(target_os = "windows")]
fn activate_notification_source_blocking(aumid: &str) -> Result<(), String> {
    use windows::core::{IUnknown, HSTRING, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_APARTMENTTHREADED, IBindCtx,
    };
    use windows::Win32::UI::Shell::{
        ApplicationActivationManager, IApplicationActivationManager, SHParseDisplayName,
        ShellExecuteExW, AO_NONE, SEE_MASK_FLAG_NO_UI, SEE_MASK_IDLIST, SEE_MASK_NOASYNC,
        SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    const UNAVAILABLE: &str = "无法定位来源应用";
    let aumid = aumid.trim();
    if aumid.is_empty() || aumid.chars().any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | ':')) {
        return Err(UNAVAILABLE.into());
    }

    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let result = (|| {
        let manager: windows::core::Result<IApplicationActivationManager> = unsafe {
            CoCreateInstance(&ApplicationActivationManager, None::<&IUnknown>, CLSCTX_ALL)
        };
        if let Ok(manager) = manager {
            if unsafe { manager.ActivateApplication(&HSTRING::from(aumid), &HSTRING::new(), AO_NONE) }.is_ok() {
                return Ok(());
            }
        }

        // Resolve the exact AppsFolder item first. ShellExecute on a string may
        // open an Explorer window even when the named application is missing.
        let target = HSTRING::from(format!("shell:AppsFolder\\{aumid}"));
        let mut item = std::ptr::null_mut();
        unsafe { SHParseDisplayName(&target, None::<&IBindCtx>, &mut item, 0, None) }
            .map_err(|_| UNAVAILABLE.to_owned())?;
        if item.is_null() {
            return Err(UNAVAILABLE.into());
        }
        let verb = HSTRING::from("open");
        let mut launch = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_IDLIST | SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpIDList: item.cast(),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        let launched = unsafe { ShellExecuteExW(&mut launch) };
        unsafe { CoTaskMemFree(Some(item.cast())) };
        launched.map_err(|_| UNAVAILABLE.to_owned())
    })();
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn activate_notification_source(app: String, aumid: String) -> Result<(), String> {
    let _ = app;
    tauri::async_runtime::spawn_blocking(move || activate_notification_source_blocking(&aumid))
        .await
        .map_err(|_| "无法打开来源应用".to_owned())?
}

/// 引导打开 Windows 专注助手设置；FlowIsle 不拦截其它应用的系统 Toast。
#[cfg(target_os = "windows")]
#[tauri::command]
fn open_focus_assist_settings() -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = HSTRING::from("open");
    let target = HSTRING::from("ms-settings:quiethours");
    let result = unsafe { ShellExecuteW(None, &operation, &target, None, None, SW_SHOWNORMAL) };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err("无法打开 Windows 专注助手设置".into())
    }
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn activate_notification_source(_app: String, _aumid: String) -> Result<(), String> {
    Err("无法定位来源应用".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn open_focus_assist_settings() -> Result<(), String> {
    Err("仅支持 Windows".into())
}

#[cfg(all(test, target_os = "windows"))]
mod notification_source_tests {
    use super::activate_notification_source_blocking;

    #[test]
    fn rejects_empty_aumid() {
        assert_eq!(
            activate_notification_source_blocking("").unwrap_err(),
            "无法定位来源应用"
        );
        assert_eq!(
            activate_notification_source_blocking("   ").unwrap_err(),
            "无法定位来源应用"
        );
    }

    #[test]
    fn rejects_unknown_aumid() {
        assert_eq!(
            activate_notification_source_blocking("Definitely.Not.Real!App").unwrap_err(),
            "无法定位来源应用"
        );
    }

    #[test]
    fn rejects_real_source_without_start_entry() {
        assert_eq!(
            activate_notification_source_blocking("Windows.Defender.SecurityCenter").unwrap_err(),
            "无法定位来源应用"
        );
    }
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn test_email_connection(
    _provider: String,
    _email: String,
    _host: String,
    _password: Option<String>,
) -> Result<String, String> {
    Err("仅支持 Windows".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn probe_notification_sources() -> Result<Vec<serde_json::Value>, String> {
    Err("仅支持 Windows".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn save_email_credential(_email: String, _password: String, _provider: String, _host: String) -> Result<(), String> {
    Err("仅支持 Windows".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn email_credential_status(_email: String, _provider: String, _host: String) -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn delete_email_credential() -> Result<(), String> {
    Err("仅支持 Windows".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 崩溃时留一份可读记录；GUI 模式下 stderr 通常不可见。
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        let path = std::env::var_os("APPDATA").map(std::path::PathBuf::from).map(|p| {
            p.join("com.focus-island.desktop")
                .join("focus-island")
                .join("panic.log")
        });
        if let Some(path) = path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(file, "---- panic ----\n{info}\n");
            }
        }
    }));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_panel(app);
        }))
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .on_tray_icon_event(|app, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    toggle_panel(app.clone());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_click_through,
            set_island_layout,
            set_peek_layout,
            clear_peek_layout,
            set_island_click_capture,
            force_current_window_frameless,
            cursor_active,
            poll_island_input,
            get_window_position,
            emit_island_panel_motion,
            position_panel_under_island,
            get_panel_transition_metrics,
            animate_panel_open,
            animate_panel_close,
            show_panel,
            hide_panel,
            toggle_panel,
            is_panel_visible,
            set_island_visible,
            get_screen_info,
            get_macos_update_health,
            get_local_audio_path,
            get_update_proxy,
            ensure_ytdlp,
            resolve_audio_url,
            import_media_entries,
            download_temp_audio,
            http_get_text,
            get_volume_state,
            set_system_volume,
            set_system_mute,
            save_email_credential,
            email_credential_status,
            delete_email_credential,
            test_email_connection,
            probe_notification_sources,
            activate_notification_source,
            open_focus_assist_settings,
        ])
        .setup(|app| {
            // 点击刘海判定依赖真实点击坐标；钩子只需要安装一次
            #[cfg(target_os = "windows")]
            mouse_hook::install();

            // 系统通知镜像：后台轮询通知中心
            #[cfg(target_os = "windows")]
            start_system_notification_mirror(app.handle().clone());

            // 邮件通知：IMAP IDLE 监听收件箱
            #[cfg(target_os = "windows")]
            start_email_watcher(app.handle().clone());

            #[cfg(desktop)]
            {
                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new().build(),
                )?;
            }

            // 全局快捷键注册 + 设置变更热更新
            #[cfg(desktop)]
            {
                use tauri::Listener;
                let shortcut_app = app.handle().clone();
                let key = read_shortcut_key(&shortcut_app);
                apply_global_shortcut(&shortcut_app, &key);
                app.listen_any("settings-changed", move |event| {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                        if let Some(k) = v.get("shortcutKey").and_then(|k| k.as_str()) {
                            apply_global_shortcut(&shortcut_app, k);
                        }
                    }
                });
            }

            if let Some(window) = app.get_webview_window("main") {
                // 配置里的 decorations:false 在部分 Windows/WebView2 组合下仍会留下 WS_CAPTION，
                // 运行时强制去掉，舞台窗口才能保持纯透明、无系统标题栏。
                let _ = window.set_title("");
                let _ = window.set_decorations(false);
                #[cfg(target_os = "windows")]
                {
                    force_frameless_window(&window);
                    disable_native_window_frame(&window, true);
                }
                if let Ok(Some(monitor)) = window.current_monitor() {
                    let screen_width = monitor.size().width as f64;
                    let win_width = window.outer_size().map(|s| s.width).unwrap_or(520) as f64;
                    let position = monitor.position();
                    let x = position.x as f64 + (screen_width - win_width) / 2.0;
                    let y = position.y as f64;
                    let _ = window.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
                }
                let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
                // WebView 缩放会跨启动保留（Ctrl+滚轮/快捷键误触），统一重置为 1，
                // 否则内容会比窗口小一圈，右侧露出第二层材质。
                let _ = window.set_zoom(1.0);
                // 舞台窗口同样不进任务栏，否则悬停任务栏会出现一张白底 + 岛体的缩略图
                let _ = window.set_skip_taskbar(true);
                #[cfg(target_os = "windows")]
                apply_window_skip_taskbar(&window);
                // 首次显示前完成无边框初始化，避免系统合成出启动标题栏。
                let _ = window.show();
            }

            if let Some(panel) = app.get_webview_window("panel") {
                let _ = panel.set_background_color(Some(Color(0, 0, 0, 0)));
                let _ = panel.set_shadow(false);
                let _ = panel.set_zoom(1.0);
                // 面板是伴随窗口，不进任务栏：否则任务栏缩略图会显示一张白页 + 标题 + 关闭按钮
                let _ = panel.set_skip_taskbar(true);
                #[cfg(target_os = "windows")]
                apply_window_skip_taskbar(&panel);
                // 窗口刚创建时扩展样式可能还被 Tauri 覆盖一次，启动后补一遍，保证收起状态也不进任务栏
                let skip_handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    if let Some(panel) = skip_handle.get_webview_window("panel") {
                        let _ = panel.set_skip_taskbar(true);
                        #[cfg(target_os = "windows")]
                        apply_window_skip_taskbar(&panel);
                    }
                    if let Some(main) = skip_handle.get_webview_window("main") {
                        let _ = main.set_skip_taskbar(true);
                        #[cfg(target_os = "windows")]
                        apply_window_skip_taskbar(&main);
                        // Tauri 在窗口 show 后偶尔会把 WS_CAPTION 加回来；舞台窗口若带标题栏，
                        // 焦点切换时 DWM 会在面板上方闪出“FlowIsle”标题条。
                        #[cfg(target_os = "windows")]
                        force_frameless_window(&main);
                    }
                });
                // 面板必须是无边框窗口：圆角/边界只由 CSS 卡片负责，不能出现系统标题栏或外框
                let _ = panel.set_decorations(false);
                // 清空原生窗口标题：即使系统在某次合成里画出了 caption，
                // 也不会再出现“专注清单”这几个字（面板自己的标题由 CSS 渲染）。
                let _ = panel.set_title("");
                #[cfg(target_os = "windows")]
                force_frameless_window(&panel);
                #[cfg(target_os = "windows")]
                disable_native_window_frame(&panel, false);
                let app_handle = app.handle().clone();
                panel.on_window_event(move |event| {
                    match event {
                        WindowEvent::Resized(size) => {
                            set_last_panel_size(*size);
                            position_panel_under_island_inner(&app_handle);
                        }
                        _ => {}
                    }
                });
            }

            let open_panel =
                MenuItem::with_id(app, "open_panel", "打开专注清单", true, None::<&str>)?;
            let toggle_island =
                MenuItem::with_id(app, "toggle_island", "显示/隐藏灵动岛", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

            let menu = Menu::with_items(app, &[&open_panel, &toggle_island, &separator, &quit])?;

            let tray_icon = Image::from_bytes(TRAY_ICON_BYTES)?;

            TrayIconBuilder::new()
                .icon(tray_icon)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open_panel" => {
                        animate_panel_open(app.clone());
                    }
                    "toggle_island" => {
                        if let Some(main) = app.get_webview_window("main") {
                            if main.is_visible().unwrap_or(true) {
                                let _ = main.hide();
                            } else {
                                let _ = main.show();
                            }
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
