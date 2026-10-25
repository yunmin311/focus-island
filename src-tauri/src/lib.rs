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
        println!("[shortcut] cleared");
        return;
    }
    let parsed = key.trim().parse::<Shortcut>();
    let Ok(sc) = parsed else {
        println!("[shortcut] invalid key: {key}");
        return;
    };
    let result = gs.on_shortcut(sc, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            toggle_panel(app.clone());
        }
    });
    match result {
        Ok(_) => println!("[shortcut] registered: {key}"),
        Err(e) => println!("[shortcut] register failed: {key} ({e})"),
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


// ==== Windows 玻璃 v2：DWM SystemBackdrop 磨砂 + 应用层深色覆盖 ====
// 教训：浅色主题下系统磨砂是白底，应用必须自己叠足够高的深色 alpha 才是深色玻璃；
// 不要用 SetWindowRgn（会破坏 WebView2 合成），圆角用 DWMWA_WINDOW_CORNER_PREFERENCE。

#[cfg(target_os = "windows")]
fn apply_dwm_round_corners<R: Runtime>(window: &WebviewWindow<R>) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DWM_WINDOW_CORNER_PREFERENCE,
    };

    let Ok(hwnd) = window.hwnd() else { return };
    let pref = DWM_WINDOW_CORNER_PREFERENCE(DWMWCP_ROUND.0);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const DWM_WINDOW_CORNER_PREFERENCE as *const core::ffi::c_void,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
    }
}

// ==== 舞台窗口：灵动岛在固定大小的透明舞台内做纯 CSS 形变 ====

#[derive(Clone, Copy, serde::Serialize)]
struct IslandLayout {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

static ISLAND_LAYOUT: std::sync::Mutex<Option<IslandLayout>> = std::sync::Mutex::new(None);

fn island_layout() -> Option<IslandLayout> {
    ISLAND_LAYOUT.lock().ok().and_then(|slot| *slot)
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
}

/// 舞台永远点击穿透，输入由该轮询判定。
/// 点击判定使用鼠标钩子记录的坐标，落在岛矩形内才算点击刘海；
/// 其他平台只用悬停判定（点击交给系统原生命中）。
#[tauri::command]
fn poll_island_input<R: Runtime>(window: WebviewWindow<R>, pad: f64) -> IslandInputState {
    let over = is_mouse_over_island_inner(&window, pad);
    #[cfg(target_os = "windows")]
    {
        let clicked = take_pending_click_inside(&window, pad);
        IslandInputState { over, clicked }
    }
    #[cfg(not(target_os = "windows"))]
    {
        IslandInputState { over, clicked: false }
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
fn show_panel<R: Runtime>(app: AppHandle<R>) {
    animate_panel_open(app);
}

#[tauri::command]
fn hide_panel<R: Runtime>(app: AppHandle<R>) {
    if let Some(panel) = app.get_webview_window("panel") {
        if let Ok(size) = panel.outer_size() {
            set_last_panel_size(size);
        }
        let _ = panel.hide();
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
    let Some((main_pos, main_size)) = island_rect(&app) else {
        return;
    };

    let (cfg_w, cfg_h, cfg_min_w, cfg_min_h) = panel_config_values(&app);
    let scale = panel.scale_factor().unwrap_or(1.0);
    let fallback_size = panel.outer_size().unwrap_or(tauri::PhysicalSize::new(
        (cfg_w * scale).round() as u32,
        (cfg_h * scale).round() as u32,
    ));
    let target_size = get_last_panel_size(fallback_size);
    let min_width = (cfg_min_w * scale).round() as u32;
    let min_height = (cfg_min_h * scale).round() as u32;
    let target_size = tauri::PhysicalSize::new(
        target_size.width.max(min_width),
        target_size.height.max(min_height),
    );
    let target_pos = panel_target_position(main_pos, main_size, target_size);

    let _ = panel.set_ignore_cursor_events(false);
    let _ = panel.set_position(target_pos);
    let _ = panel.set_size(target_size);
    let _ = panel.set_min_size(Some(tauri::LogicalSize::new(cfg_min_w, cfg_min_h)));

    if let Some(main) = app.get_webview_window("main") {
        if !main.is_visible().unwrap_or(true) && !island_user_hidden() {
            let _ = main.show();
        }
    }
    let _ = panel.show();
    let _ = panel.emit("panel-window-transition", "open");
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
    if panel.emit("panel-window-transition", "close").is_err() {
        let _ = panel.hide();
    }
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
                    println!("[system-notif] listener unavailable: {e}");
                    return;
                }
            };

            match listener.RequestAccessAsync().and_then(|op| op.get()) {
                Ok(UserNotificationListenerAccessStatus::Allowed) => {
                    println!("[system-notif] access allowed");
                }
                Ok(status) => {
                    println!("[system-notif] access not allowed: {status:?}");
                    return;
                }
                Err(e) => {
                    println!("[system-notif] access request failed: {e}");
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
                            println!("[system-notif] poll failed: {msg}");
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
        println!("[system-notif] failed to start mirror thread: {e}");
    }
}

// ==== 邮件通知（Windows）：IMAP 只取发件人/主题/时间 ====
// 凭据存 Windows 凭据管理器；QQ/163 用授权码，Gmail 用应用专用密码。
// 默认 IDLE 实时监听，服务器不支持时退化为 60s 轮询。

#[cfg(target_os = "windows")]
const EMAIL_CRED_TARGET: &str = "FocusIsland/Email";

#[cfg(target_os = "windows")]
#[derive(Clone, PartialEq)]
struct EmailConfig {
    enabled: bool,
    provider: String,
    address: String,
}

#[cfg(target_os = "windows")]
#[derive(Clone, serde::Serialize)]
struct EmailNotificationEvent {
    uid: u32,
    from: String,
    subject: String,
    date: String,
    provider: String,
}

#[cfg(target_os = "windows")]
type ImapSession = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

#[cfg(target_os = "windows")]
fn email_imap_host(provider: &str) -> Option<&'static str> {
    match provider {
        "qq" => Some("imap.qq.com"),
        "163" => Some("imap.163.com"),
        "gmail" => Some("imap.gmail.com"),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
fn save_email_credential_blocking(email: &str, password: &str) -> Result<(), String> {
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

    let credential = CREDENTIALW {
        Flags: Default::default(),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        Comment: PWSTR::null(),
        LastWritten: Default::default(),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: PWSTR::null(),
        UserName: PWSTR(user.as_mut_ptr()),
    };

    unsafe { CredWriteW(&credential, 0) }.map_err(|e| format!("保存凭据失败: {e}"))
}

#[cfg(target_os = "windows")]
fn read_email_credential_blocking() -> Option<(String, String)> {
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
    unsafe { CredFree(credential_ptr as *const _) };
    Some((user, password))
}

#[cfg(target_os = "windows")]
fn delete_email_credential_blocking() {
    use windows::core::PCWSTR;
    use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
    let target: Vec<u16> = EMAIL_CRED_TARGET
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let _ = unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) };
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
    })
}

#[cfg(target_os = "windows")]
fn same_email_config<R: Runtime>(app: &AppHandle<R>, config: &EmailConfig) -> bool {
    read_email_config(app).as_ref() == Some(config)
}

#[cfg(target_os = "windows")]
fn extract_email_envelope(fetch: &imap::types::Fetch) -> (String, String, String) {
    let Some(envelope) = fetch.envelope() else {
        return ("未知发件人".into(), "(无主题)".into(), String::new());
    };

    let subject = envelope
        .subject
        .map(|s| String::from_utf8_lossy(s).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(无主题)".to_string());

    let from = envelope
        .from
        .as_ref()
        .and_then(|list| list.first())
        .map(|addr| {
            let name = addr
                .name
                .map(|n| String::from_utf8_lossy(n).trim().to_string())
                .unwrap_or_default();
            if !name.is_empty() {
                return name;
            }
            let mailbox = addr
                .mailbox
                .map(|m| String::from_utf8_lossy(m).to_string())
                .unwrap_or_default();
            let host = addr
                .host
                .map(|h| String::from_utf8_lossy(h).to_string())
                .unwrap_or_default();
            if mailbox.is_empty() && host.is_empty() {
                "未知发件人".to_string()
            } else {
                format!("{mailbox}@{host}")
            }
        })
        .unwrap_or_else(|| "未知发件人".to_string());

    let date = envelope
        .date
        .map(|d| String::from_utf8_lossy(d).trim().to_string())
        .unwrap_or_default();

    (from, subject, date)
}

#[cfg(target_os = "windows")]
fn fetch_new_email<R: Runtime>(
    app: &AppHandle<R>,
    session: &mut ImapSession,
    seen: &mut std::collections::HashSet<u32>,
    seeded: &mut bool,
    provider: &str,
) -> Result<(), String> {
    let unread = session
        .uid_search("UNSEEN")
        .map_err(|e| format!("搜索未读失败: {e}"))?;

    if !*seeded {
        for uid in unread {
            seen.insert(uid);
        }
        *seeded = true;
        println!("[email] seeded {} unread", seen.len());
        return Ok(());
    }

    let new_uids: Vec<u32> = unread
        .iter()
        .copied()
        .filter(|uid| !seen.contains(uid))
        .collect();
    if new_uids.is_empty() {
        return Ok(());
    }

    let uid_set = new_uids
        .iter()
        .map(|uid| uid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let fetches = session
        .uid_fetch(&uid_set, "(UID ENVELOPE INTERNALDATE)")
        .map_err(|e| format!("获取邮件头失败: {e}"))?;

    for fetch in fetches.iter() {
        let uid = fetch.uid.unwrap_or(0);
        seen.insert(uid);
        let (from, subject, date) = extract_email_envelope(fetch);
        let event = EmailNotificationEvent {
            uid,
            from,
            subject,
            date,
            provider: provider.to_string(),
        };
        let _ = app.emit("email-notification", event);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_email_imap_session<R: Runtime>(
    app: &AppHandle<R>,
    config: &EmailConfig,
    password: &str,
    seen: &mut std::collections::HashSet<u32>,
    seeded: &mut bool,
) -> Result<(), String> {
    let host = email_imap_host(&config.provider).ok_or_else(|| "不支持的邮箱服务商".to_string())?;
    let tls = native_tls::TlsConnector::new().map_err(|e| format!("TLS 初始化失败: {e}"))?;
    let client = imap::connect((host, 993), host, &tls)
        .map_err(|e| format!("连接 {host} 失败: {e}"))?;
    let mut session = client
        .login(&config.address, password)
        .map_err(|e| format!("登录失败: {}", e.0))?;

    // 网易邮箱（163/QQ）要求客户端先发送 ID 命令
    if config.provider == "163" || config.provider == "qq" {
        let _ = session
            .run_command_and_check_ok("ID (\"name\" \"FocusIsland\" \"version\" \"2.9.0\")");
    }

    session
        .select("INBOX")
        .map_err(|e| format!("选择收件箱失败: {e}"))?;
    println!("[email] connected: {} ({})", config.address, config.provider);

    fetch_new_email(app, &mut session, seen, seeded, &config.provider)?;

    let supports_idle = session
        .capabilities()
        .map(|caps| caps.has_str("IDLE"))
        .unwrap_or(false);

    if supports_idle {
        loop {
            let outcome = {
                let handle = session.idle().map_err(|e| format!("IDLE 失败: {e}"))?;
                handle
                    .wait_with_timeout(Duration::from_secs(60))
                    .map_err(|e| format!("IDLE 等待失败: {e}"))?
            };

            if !same_email_config(app, config) {
                let _ = session.logout();
                return Ok(());
            }

            if outcome == imap::extensions::idle::WaitOutcome::MailboxChanged {
                fetch_new_email(app, &mut session, seen, seeded, &config.provider)?;
            }
        }
    } else {
        loop {
            std::thread::sleep(Duration::from_secs(60));
            if !same_email_config(app, config) {
                let _ = session.logout();
                return Ok(());
            }
            fetch_new_email(app, &mut session, seen, seeded, &config.provider)?;
        }
    }
}

#[cfg(target_os = "windows")]
fn start_email_watcher<R: Runtime>(app: AppHandle<R>) {
    let spawned = std::thread::Builder::new()
        .name("email-imap".into())
        .spawn(move || {
            let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
            let mut seeded = false;
            loop {
                let Some(config) = read_email_config(&app) else {
                    std::thread::sleep(Duration::from_secs(10));
                    continue;
                };
                if !config.enabled || config.address.is_empty() {
                    seen.clear();
                    seeded = false;
                    std::thread::sleep(Duration::from_secs(10));
                    continue;
                }
                let Some((stored_user, password)) = read_email_credential_blocking() else {
                    std::thread::sleep(Duration::from_secs(30));
                    continue;
                };
                if !stored_user.eq_ignore_ascii_case(&config.address) {
                    std::thread::sleep(Duration::from_secs(30));
                    continue;
                }
                if let Err(e) = run_email_imap_session(
                    &app,
                    &config,
                    &password,
                    &mut seen,
                    &mut seeded,
                ) {
                    println!("[email] {e}");
                    std::thread::sleep(Duration::from_secs(30));
                }
            }
        });

    if let Err(e) = spawned {
        println!("[email] failed to start watcher: {e}");
    }
}

#[cfg(target_os = "windows")]
fn test_email_connection_blocking(provider: &str, email: &str) -> Result<String, String> {
    let email = email.trim();
    if email.is_empty() {
        return Err("请先填写邮箱地址".into());
    }
    let Some((stored_user, password)) = read_email_credential_blocking() else {
        return Err("请先保存授权码".into());
    };
    if !stored_user.eq_ignore_ascii_case(email) {
        return Err("授权码与当前邮箱不匹配，请重新保存".into());
    }
    let host = email_imap_host(provider).ok_or_else(|| "不支持的邮箱服务商".to_string())?;
    let tls = native_tls::TlsConnector::new().map_err(|e| format!("TLS 初始化失败: {e}"))?;
    let client = imap::connect((host, 993), host, &tls)
        .map_err(|e| format!("连接 {host} 失败: {e}"))?;
    let mut session = client
        .login(email, &password)
        .map_err(|e| format!("登录失败: {}", e.0))?;
    if provider == "163" || provider == "qq" {
        let _ = session
            .run_command_and_check_ok("ID (\"name\" \"FocusIsland\" \"version\" \"2.9.0\")");
    }
    session
        .select("INBOX")
        .map_err(|e| format!("收件箱不可用: {e}"))?;
    let _ = session.logout();
    Ok("连接成功".to_string())
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn save_email_credential(email: String, password: String) -> Result<(), String> {
    let email = email.trim();
    if email.is_empty() || password.is_empty() {
        return Err("请填写邮箱地址和授权码".into());
    }
    save_email_credential_blocking(email, &password)
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn email_credential_status(email: String) -> Result<bool, String> {
    Ok(read_email_credential_blocking()
        .map(|(stored_user, _)| stored_user.eq_ignore_ascii_case(email.trim()))
        .unwrap_or(false))
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn delete_email_credential() -> Result<(), String> {
    delete_email_credential_blocking();
    Ok(())
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn test_email_connection(provider: String, email: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        test_email_connection_blocking(&provider, &email)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn save_email_credential(_email: String, _password: String) -> Result<(), String> {
    Err("仅支持 Windows".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn email_credential_status(_email: String) -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn delete_email_credential() -> Result<(), String> {
    Err("仅支持 Windows".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn test_email_connection(_provider: String, _email: String) -> Result<String, String> {
    Err("仅支持 Windows".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
                if let Ok(Some(monitor)) = window.current_monitor() {
                    let screen_width = monitor.size().width as f64;
                    let win_width = window.outer_size().map(|s| s.width).unwrap_or(520) as f64;
                    let position = monitor.position();
                    let x = position.x as f64 + (screen_width - win_width) / 2.0;
                    let y = position.y as f64;
                    let _ = window.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
                }
                let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
                // 注意：舞台窗口不能开系统 Acrylic——刘海只覆盖舞台一小部分，
                // 磨砂会把整个窗口矩形染成一块可见的毛玻璃板（浅色主题尤其明显）
            }

            if let Some(panel) = app.get_webview_window("panel") {
                let _ = panel.set_background_color(Some(Color(0, 0, 0, 0)));
                let _ = panel.set_shadow(false);
                #[cfg(target_os = "windows")]
                apply_dwm_round_corners(&panel);
                let app_handle = app.handle().clone();
                panel.on_window_event(move |event| {
                    if let WindowEvent::Resized(size) = event {
                        set_last_panel_size(*size);
                        position_panel_under_island_inner(&app_handle);
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
