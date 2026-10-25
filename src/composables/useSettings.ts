import { ref, computed, watch, watchEffect } from 'vue'
import { readTextFile, writeTextFile, mkdir, BaseDirectory } from '@tauri-apps/plugin-fs'
import { emit } from '@tauri-apps/api/event'
import { enable, disable, isEnabled } from '@tauri-apps/plugin-autostart'

export interface ThemePreset {
  id: string
  name: string
  focusColor: string
  breakColor: string
  idleColor: string
  alertColor: string
}

export type ColorMode = 'dark' | 'light' | 'system'

export type IslandDisplayMode = 'resident' | 'dock' | 'hidden'

export type SystemNotificationMirrorMode = 'all' | 'selected'

export type EmailProvider = 'qq' | '163' | 'gmail'

export interface Settings {
  islandOpacity: number      // 灵动岛透明度 0~0.7，值越大越透明，CSS alpha = 1 - value
  panelOpacity: number       // 面板透明度 0~0.9，同上
  islandScale: number        // 灵动岛缩放比例 0.5~1.5
  islandDisplayMode: IslandDisplayMode  // 灵动岛显示方式：常驻 / 边缘停靠 / 完全隐藏
  notchClickOpensPanel: boolean // 点击刘海/胶囊是否打开面板（关闭则仅快捷键/托盘）
  focusDuration: number
  breakDuration: number
  colorMode: ColorMode
  activeThemeId: string
  idleMottos: string[]       // 空闲时轮播的激励语，空数组则用内置默认
  shortcutKey: string        // 全局快捷键，如 "Alt+Space"
  autoCheckUpdates: boolean  // 启动后自动检查更新
  notificationSound: boolean // 通知提示音开关
  autoLaunch: boolean       // 开机自动启动
  systemNotificationMirror: boolean     // 镜像 Windows 系统通知到通知中心与岛内 peek
  systemNotificationMirrorMode: SystemNotificationMirrorMode // all=全部（可屏蔽）；selected=只镜像允许列表
  systemNotificationMutedApps: string[] // 全部模式下不镜像的来源（应用名或 AUMID）
  systemNotificationAllowedApps: string[] // 只镜像模式下允许的来源（应用名或 AUMID）
  systemNotificationSilentInFocus: boolean // 番茄专注运行中只记录、不弹岛内 peek
  emailEnabled: boolean        // 邮件提醒开关
  emailProvider: EmailProvider // QQ / 163 / Gmail
  emailAddress: string         // 邮箱地址（授权码存 Windows 凭据管理器）
}

export const presetThemes: ThemePreset[] = [
  { id: 'classic', name: '经典橙红', focusColor: '#e85d3a', breakColor: '#3a9e6e', idleColor: '#4a8fd4', alertColor: '#e8a23a' },
  { id: 'ocean', name: '海洋蓝绿', focusColor: '#2d8cf0', breakColor: '#36b37e', idleColor: '#6b8eb2', alertColor: '#f5a623' },
  { id: 'lavender', name: '薰衣草紫', focusColor: '#9b6dff', breakColor: '#4ecdc4', idleColor: '#7c8ba1', alertColor: '#f7b731' },
  { id: 'forest', name: '森林深绿', focusColor: '#27ae60', breakColor: '#3498db', idleColor: '#8e9aaf', alertColor: '#e67e22' },
  { id: 'cyber', name: '赛博朋克', focusColor: '#ff6b9d', breakColor: '#00d2d3', idleColor: '#a29bfe', alertColor: '#feca57' },
]

const SETTINGS_FILE = 'focus-island/settings.json'

const DEFAULT_SETTINGS: Settings = {
  islandOpacity: 0.25,
  panelOpacity: 0.25,
  islandScale: 1.0,
  islandDisplayMode: 'dock',
  notchClickOpensPanel: true,
  focusDuration: 25,
  breakDuration: 5,
  colorMode: 'system',
  activeThemeId: 'classic',
  idleMottos: [],
  shortcutKey: 'Alt+Space',
  autoCheckUpdates: true,
  notificationSound: true,
  autoLaunch: true,
  systemNotificationMirror: true,
  systemNotificationMirrorMode: 'all',
  systemNotificationMutedApps: [],
  systemNotificationAllowedApps: [],
  systemNotificationSilentInFocus: true,
  emailEnabled: false,
  emailProvider: 'qq',
  emailAddress: '',
}

const settings = ref<Settings>({ ...DEFAULT_SETTINGS })
const loaded = ref(false)
let suppressEmit = false
let systemSchemeListenerBound = false

function normalizeColorMode(mode: unknown): ColorMode {
  if (mode === 'dark' || mode === 'light' || mode === 'system') return mode
  return 'system'
}

export function resolveColorMode(mode: ColorMode): 'dark' | 'light' {
  if (mode !== 'system') return mode
  if (typeof window !== 'undefined' && typeof window.matchMedia === 'function') {
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  }
  return 'dark'
}

/** 系统通知镜像是否应处理这条通知（总开关、自身通知、来源过滤） */
export function shouldMirrorSystemNotification(app: string, aumid: string): boolean {
  if (!settings.value.systemNotificationMirror) return false
  const appName = (app ?? '').trim().toLowerCase()
  const appId = (aumid ?? '').trim().toLowerCase()
  if (appId.includes('com.focus-island')) return false

  const matches = (entry: string) => {
    const key = entry.trim().toLowerCase()
    return key.length > 0 && (key === appName || key === appId)
  }

  if ((settings.value.systemNotificationMirrorMode ?? 'all') === 'selected') {
    const allowed = settings.value.systemNotificationAllowedApps ?? []
    if (allowed.length === 0) return false
    return allowed.some(matches)
  }

  const muted = settings.value.systemNotificationMutedApps ?? []
  return !muted.some(matches)
}

function onSystemSchemeChange() {
  if (settings.value.colorMode === 'system') applyThemeToDOM()
}

function ensureSystemSchemeListener() {
  if (systemSchemeListenerBound) return
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return

  const mq = window.matchMedia('(prefers-color-scheme: dark)')
  if (typeof mq.addEventListener === 'function') {
    mq.addEventListener('change', onSystemSchemeChange)
  } else if (typeof mq.addListener === 'function') {
    mq.addListener(onSystemSchemeChange)
  }
  systemSchemeListenerBound = true
}

function applyThemeToDOM() {
  const theme = presetThemes.find(t => t.id === settings.value.activeThemeId) ?? presetThemes[0]
  const root = document.documentElement
  const resolvedColorMode = resolveColorMode(settings.value.colorMode)
  root.dataset.colorMode = resolvedColorMode
  // 浅色面板通过 --panel-root-filter 反色实现；原生滚动条 / range 滑块
  // 必须保持 dark 配色，反色后才会呈现浅色，否则会残留成黑色。
  root.style.colorScheme = 'dark'
  root.style.setProperty('--focus-color', theme.focusColor)
  root.style.setProperty('--break-color', theme.breakColor)
  root.style.setProperty('--idle-color', theme.idleColor)
  root.style.setProperty('--alert-color', theme.alertColor)
  root.style.setProperty('--island-opacity', String(1 - settings.value.islandOpacity))
  root.style.setProperty('--panel-opacity', String(1 - settings.value.panelOpacity))
  root.style.setProperty('--island-scale', String(settings.value.islandScale))
}

async function load() {
  try {
    const raw = await readTextFile(SETTINGS_FILE, { baseDir: BaseDirectory.AppData })
    const parsed = JSON.parse(raw) as Partial<Settings>
    settings.value = {
      ...DEFAULT_SETTINGS,
      ...parsed,
      colorMode: normalizeColorMode(parsed.colorMode),
    }
  } catch {
    settings.value = { ...DEFAULT_SETTINGS }
  }
  loaded.value = true
  // Sync autoLaunch with OS autostart state
  try {
    const enabled = await isEnabled()
    settings.value.autoLaunch = enabled
  } catch {
    // autostart plugin unavailable (e.g. unsupported platform)
  }
  if (settings.value.autoLaunch) {
    enable().catch(() => {})
  }
}

async function save() {
  if (!loaded.value) return
  try {
    await mkdir('focus-island', { baseDir: BaseDirectory.AppData, recursive: true })
    await writeTextFile(SETTINGS_FILE, JSON.stringify(settings.value), { baseDir: BaseDirectory.AppData })
  } catch (e) {
    console.error('Failed to save settings', e)
  }
}

watch(settings, () => {
  if (!suppressEmit) {
    save()
    emit('settings-changed', settings.value)
  }
  suppressEmit = false
}, { deep: true })

watch(() => settings.value.autoLaunch, (val) => {
  if (!loaded.value) return
  if (val) {
    enable().catch(() => { settings.value.autoLaunch = false })
  } else {
    disable().catch(() => {})
  }
})

watchEffect(() => {
  if (loaded.value) applyThemeToDOM()
})

const activeTheme = computed(() =>
  presetThemes.find(t => t.id === settings.value.activeThemeId) ?? presetThemes[0]
)

function applyExternalSettings(newSettings: Settings) {
  suppressEmit = true
  settings.value = {
    ...newSettings,
    colorMode: normalizeColorMode(newSettings.colorMode),
  }
  loaded.value = true
}

export function useSettings() {
  ensureSystemSchemeListener()
  if (!loaded.value) load()

  function resetSettings() {
    settings.value = { ...DEFAULT_SETTINGS }
    applyThemeToDOM()
  }

  return { settings, settingsReady: loaded, activeTheme, applyThemeToDOM, applyExternalSettings, resetSettings }
}
