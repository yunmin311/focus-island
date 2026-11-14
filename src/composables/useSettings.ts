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

/** 纯 CSS 面板材质：亮面、主题磨砂、哑光亚克力。 */
export type PanelGlass = 'bright' | 'frost' | 'acrylic'

export type SystemNotificationMirrorMode = 'all' | 'selected'

export type EmailProvider = 'qq' | '163' | '126' | 'gmail' | 'outlook' | 'custom'

export interface Settings {
  islandOpacity: number      // 灵动岛不透明度 0.4~1.0，值越大越不透明
  panelOpacity: number       // 面板不透明度 0.4~1.0，值越大越不透明
  islandScale: number        // 灵动岛缩放比例 0.5~1.5
  islandDisplayMode: IslandDisplayMode  // 灵动岛显示方式：常驻 / 边缘停靠 / 完全隐藏
  panelGlass: PanelGlass      // 面板玻璃质感
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
  systemNotificationKnownSources: string[] // 自动/探针发现的来源，持久保留
  systemNotificationSilentInFocus: boolean // 番茄专注运行中暂停系统和邮件的岛内提示及通知记录
  emailEnabled: boolean        // 邮件提醒开关
  emailProvider: EmailProvider // QQ / 163 / 126 / Gmail / Outlook / 自定义
  emailAddress: string         // 邮箱地址（授权码存 Windows 凭据管理器）
  emailImapHost: string        // 自定义邮箱的 IMAP 服务器
  settingsVersion: number      // 迁移版本；2 = 透明度改为不透明度语义
}

export const presetThemes: ThemePreset[] = [
  { id: 'classic', name: '经典橙红', focusColor: '#b34127', breakColor: '#537b51', idleColor: '#4a8fd4', alertColor: '#e8a23a' },
  { id: 'ocean', name: '海洋蓝绿', focusColor: '#176b86', breakColor: '#2f7a63', idleColor: '#6b8eb2', alertColor: '#c77d22' },
  { id: 'lavender', name: '薰衣草紫', focusColor: '#79509e', breakColor: '#6f7f5a', idleColor: '#7c8ba1', alertColor: '#c98a20' },
  { id: 'forest', name: '森林深绿', focusColor: '#3f7043', breakColor: '#3f7043', idleColor: '#8e9aaf', alertColor: '#b46e20' },
  { id: 'cyber', name: '赛博朋克', focusColor: '#a8357c', breakColor: '#2f7a6a', idleColor: '#a29bfe', alertColor: '#b8791e' },
  { id: 'liquid', name: 'Liquid Focus', focusColor: '#ad461e', breakColor: '#537b51', idleColor: '#776253', alertColor: '#c96a32' },
]

const SETTINGS_FILE = 'focus-island/settings.json'

const DEFAULT_SETTINGS: Settings = {
  islandOpacity: 0.92,
  panelOpacity: 0.92,
  islandScale: 1.0,
  islandDisplayMode: 'dock',
  panelGlass: 'frost',
  notchClickOpensPanel: true,
  focusDuration: 25,
  breakDuration: 5,
  colorMode: 'system',
  activeThemeId: 'liquid',
  idleMottos: [],
  shortcutKey: 'Alt+Space',
  autoCheckUpdates: true,
  notificationSound: true,
  autoLaunch: true,
  systemNotificationMirror: true,
  systemNotificationMirrorMode: 'all',
  systemNotificationMutedApps: [],
  systemNotificationAllowedApps: [],
  systemNotificationKnownSources: [],
  systemNotificationSilentInFocus: true,
  emailEnabled: false,
  emailProvider: 'qq',
  emailAddress: '',
  emailImapHost: '',
  settingsVersion: 2,
}

const settings = ref<Settings>({ ...DEFAULT_SETTINGS })
const loaded = ref(false)
let suppressEmit = false
let systemSchemeListenerBound = false

function normalizeColorMode(mode: unknown): ColorMode {
  if (mode === 'dark' || mode === 'light' || mode === 'system') return mode
  return 'system'
}

function normalizePanelGlass(mode: unknown): PanelGlass {
  if (mode === 'bright' || mode === 'frost' || mode === 'acrylic') return mode
  return 'frost'
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

/** 记住探针/事件里出现过的系统通知来源，供设置页长期展示 */
export function rememberSystemNotificationSource(app: string, aumid: string) {
  if (!settings.value.systemNotificationMirror) return
  const name = (app || aumid || '').trim()
  if (!name) return
  const list = settings.value.systemNotificationKnownSources ?? []
  if (list.some(entry => entry.toLowerCase() === name.toLowerCase())) return
  settings.value.systemNotificationKnownSources = [...list, name].slice(-80)
}

/** 通知记录对应的来源键：优先应用名，其次 AUMID */
export function systemNotificationSourceKey(app?: string, aumid?: string): string {
  return (app ?? '').trim() || (aumid ?? '').trim()
}

/** 当前模式下该来源是否已被排除（all=黑名单；selected=白名单） */
export function isSystemNotificationSourceBlocked(app: string, aumid: string): boolean {
  const appName = (app ?? '').trim().toLowerCase()
  const appId = (aumid ?? '').trim().toLowerCase()
  if (!appName && !appId) return false

  const matches = (entry: string) => {
    const key = entry.trim().toLowerCase()
    return key.length > 0 && (key === appName || key === appId)
  }

  if ((settings.value.systemNotificationMirrorMode ?? 'all') === 'selected') {
    const allowed = settings.value.systemNotificationAllowedApps ?? []
    return !allowed.some(matches)
  }

  const muted = settings.value.systemNotificationMutedApps ?? []
  return muted.some(matches)
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

function hexToRgb(value: string): [number, number, number] {
  const hex = value.trim().replace('#', '')
  if (hex.length === 6 || hex.length === 8) {
    return [
      parseInt(hex.slice(0, 2), 16),
      parseInt(hex.slice(2, 4), 16),
      parseInt(hex.slice(4, 6), 16),
    ]
  }
  return [255, 250, 240]
}

function applyThemeToDOM() {
  const theme = presetThemes.find(t => t.id === settings.value.activeThemeId) ?? presetThemes[5]
  const root = document.documentElement
  const resolvedColorMode = resolveColorMode(settings.value.colorMode)
  root.dataset.colorMode = resolvedColorMode
  root.dataset.themeId = theme.id
  root.style.setProperty('--island-scale', String(settings.value.islandScale))

  // 单层玻璃：透明度只算在外壳上；内容卡片用 theme.css 里的轻叠加，不再各自变成半透明层
  const computed = getComputedStyle(root)
  const shell = hexToRgb(computed.getPropertyValue('--v2-shell'))
  const panelOpacity = Math.min(1, Math.max(0.4, settings.value.panelOpacity))
  const opacityProgress = (panelOpacity - 0.4) / 0.6
  const islandAlpha = Math.min(1, Math.max(0.4, settings.value.islandOpacity))
  const glass = settings.value.panelGlass
  const dark = resolvedColorMode === 'dark'

  // 三种材质从不同透光量起步，滑条到顶时外壳统一完全不透明。
  const panelAlpha = glass === 'bright'
    ? 0.40 + 0.60 * opacityProgress ** 1.25
    : glass === 'frost'
      ? 0.47 + 0.53 * opacityProgress ** 1.1
      : 0.55 + 0.45 * opacityProgress ** 0.95

  // 取主题光晕的色值，外壳的透明度独立由滑条控制。
  const glow = hexToRgb(computed.getPropertyValue('--v2-glow'))
  const glowMix = glass === 'frost' ? 0.13 : dark ? 0.28 : 0.36
  const panelColor = glass === 'bright'
    ? shell.map(channel => Math.round(channel + (255 - channel) * (dark ? 0.11 : 0.36)))
    : shell.map((channel, i) => Math.round(channel + (glow[i] - channel) * glowMix))
  root.style.setProperty('--panel-shell-bg', `rgba(${panelColor.join(', ')}, ${panelAlpha})`)
  root.style.setProperty('--island-shell-bg', `rgba(${shell[0]}, ${shell[1]}, ${shell[2]}, ${islandAlpha})`)
}

async function load() {
  try {
    const raw = await readTextFile(SETTINGS_FILE, { baseDir: BaseDirectory.AppData })
    const parsed = JSON.parse(raw) as Partial<Settings>
    const legacyOpacity = (parsed.settingsVersion ?? 1) < 2
    settings.value = {
      ...DEFAULT_SETTINGS,
      ...parsed,
      colorMode: normalizeColorMode(parsed.colorMode),
      panelGlass: normalizePanelGlass(parsed.panelGlass),
      ...(legacyOpacity ? { islandOpacity: 0.92, panelOpacity: 0.92, settingsVersion: 2 } : {}),
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
