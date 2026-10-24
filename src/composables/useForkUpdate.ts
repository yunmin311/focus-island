import { computed, ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-shell'

// YM 版更新检查：只查自己的 Release 并跳转下载，不做应用内替换
const REPO = 'yunmin311/focus-island'
const API_URL = `https://api.github.com/repos/${REPO}/releases/latest`
// API 有 60 次/小时限额且国内常被 403；Atom 订阅没有限额，优先使用
const ATOM_URL = `https://github.com/${REPO}/releases.atom`
const RELEASES_URL = `https://github.com/${REPO}/releases/latest`
const CHECK_INTERVAL_MS = 12 * 60 * 60 * 1000
const LAST_CHECK_KEY = 'focus-island:fork-update-check'

const checking = ref(false)
const checked = ref(false)
const available = ref(false)
const latestTag = ref('')
const latestUrl = ref('')
const error = ref<string | null>(null)
const currentVersion = ref('')

function parseVersion(v: string): number[] {
  return v
    .replace(/^v/i, '')
    .split(/[+-]/)[0]
    .split('.')
    .map(n => Number.parseInt(n, 10) || 0)
}

function isNewer(latest: string, current: string): boolean {
  const a = parseVersion(latest)
  const b = parseVersion(current)
  const len = Math.max(a.length, b.length)
  for (let i = 0; i < len; i += 1) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0)
    if (diff !== 0) return diff > 0
  }
  return false
}

async function loadCurrentVersion() {
  if (currentVersion.value) return
  try {
    currentVersion.value = await getVersion()
  } catch {
    currentVersion.value = ''
  }
}

async function fetchLatestFromAtom(): Promise<{ tag: string; url: string } | null> {
  const xml = await invoke<string>('http_get_text', { url: ATOM_URL })
  const entry = xml.match(/<entry>([\s\S]*?)<\/entry>/i)?.[1]
  if (!entry) return null

  // id 形如 tag:github.com,2008:Repository/1430644631/v2.9.0
  const tag = entry.match(/<id>[^<]*?\/([^/<]+)<\/id>/i)?.[1] ?? ''
  if (!tag) return null

  const link =
    entry.match(/<link[^>]*rel="alternate"[^>]*href="([^"]+)"/i)?.[1]
    ?? entry.match(/<link[^>]*href="([^"]+)"[^>]*rel="alternate"/i)?.[1]
    ?? RELEASES_URL

  return { tag, url: link }
}

async function fetchLatestFromApi(): Promise<{ tag: string; url: string } | null> {
  const res = await fetch(API_URL, { headers: { Accept: 'application/vnd.github+json' } })
  if (res.status === 404) return null
  if (res.status === 403) throw new Error('GitHub 访问受限（403），请稍后重试')
  if (!res.ok) throw new Error(`GitHub ${res.status}`)
  const data = (await res.json()) as { tag_name?: string; html_url?: string }
  if (!data.tag_name) return null
  return { tag: data.tag_name, url: data.html_url || RELEASES_URL }
}

export async function checkForkUpdate(force = false) {
  if (checking.value) return
  const last = Number(localStorage.getItem(LAST_CHECK_KEY) || 0)
  if (!force && Date.now() - last < CHECK_INTERVAL_MS) return

  checking.value = true
  error.value = null
  try {
    await loadCurrentVersion()

    let latest: { tag: string; url: string } | null = null
    try {
      latest = await fetchLatestFromAtom()
    } catch {
      // Atom 拉取失败时再试 API
      latest = await fetchLatestFromApi()
    }

    localStorage.setItem(LAST_CHECK_KEY, String(Date.now()))
    checked.value = true

    if (!latest) {
      available.value = false
      return
    }

    latestTag.value = latest.tag
    latestUrl.value = latest.url
    available.value =
      !!latestTag.value && !!currentVersion.value && isNewer(latestTag.value, currentVersion.value)
  } catch (e) {
    const msg = typeof e === 'string' ? e : (e as Error).message || '检查失败'
    error.value = msg.includes('403')
      ? 'GitHub 访问受限（403），请稍后重试'
      : '网络无法访问 GitHub，请稍后重试'
  } finally {
    checking.value = false
  }
}

export function openLatestRelease() {
  void open(latestUrl.value || RELEASES_URL)
}

export function useForkUpdate() {
  const statusText = computed(() => {
    if (checking.value) return '正在检查…'
    if (error.value) return `检查失败：${error.value}`
    if (!checked.value) return '尚未检查'
    if (available.value) return `发现新版本 ${latestTag.value}`
    return currentVersion.value ? `已是最新（v${currentVersion.value}）` : '已是最新'
  })

  return {
    checking,
    checked,
    available,
    latestTag,
    latestUrl,
    error,
    currentVersion,
    statusText,
    check: checkForkUpdate,
    openLatest: openLatestRelease,
  }
}
