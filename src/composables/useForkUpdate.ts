import { computed, ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { open } from '@tauri-apps/plugin-shell'

// YM 版更新检查：只查自己的 Release 并跳转下载，不做应用内替换
const REPO = 'yunmin311/focus-island'
const API_URL = `https://api.github.com/repos/${REPO}/releases/latest`
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

export async function checkForkUpdate(force = false) {
  if (checking.value) return
  const last = Number(localStorage.getItem(LAST_CHECK_KEY) || 0)
  if (!force && Date.now() - last < CHECK_INTERVAL_MS) return

  checking.value = true
  error.value = null
  try {
    await loadCurrentVersion()
    const res = await fetch(API_URL, { headers: { Accept: 'application/vnd.github+json' } })
    localStorage.setItem(LAST_CHECK_KEY, String(Date.now()))

    if (res.status === 404) {
      checked.value = true
      available.value = false
      return
    }
    if (!res.ok) throw new Error(`GitHub ${res.status}`)

    const data = (await res.json()) as { tag_name?: string; html_url?: string }
    latestTag.value = data.tag_name || ''
    latestUrl.value = data.html_url || RELEASES_URL
    available.value =
      !!latestTag.value && !!currentVersion.value && isNewer(latestTag.value, currentVersion.value)
    checked.value = true
  } catch (e) {
    error.value = typeof e === 'string' ? e : (e as Error).message || '检查失败'
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
