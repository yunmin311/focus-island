import { ref, computed } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { readTextFile, writeTextFile, mkdir, BaseDirectory } from '@tauri-apps/plugin-fs'
import { shouldMirrorSystemNotification } from './useSettings'

export type NotificationKind = 'focus-done' | 'break-done' | 'system' | 'task-reminder'

export interface NotificationRecord {
  id: string
  kind: NotificationKind
  title: string
  detail: string
  time: number
  read: boolean
  /** 系统通知镜像：来源应用名 */
  app?: string
  /** 系统通知镜像：来源应用 AUMID */
  aumid?: string
}

/** Rust 端 system-notification 事件的载荷 */
export interface SystemNotificationPayload {
  id: number
  app: string
  aumid: string
  title: string
  body: string
  created_ms: number
}

const NOTIF_FILE = 'focus-island/notifications.json'
const LIMIT = 200
// 系统通知按系统逻辑短期缓存；专注提醒保留更久
const SYSTEM_TTL_MS = 7 * 24 * 60 * 60 * 1000
const FOCUS_TTL_MS = 30 * 24 * 60 * 60 * 1000

const records = ref<NotificationRecord[]>([])
const loaded = ref(false)
let listening = false
const systemKeys = new Set<string>()

function pruneByRetention(list: NotificationRecord[]): NotificationRecord[] {
  const now = Date.now()
  return list
    .filter((record) => {
      const ttl = record.kind === 'system' ? SYSTEM_TTL_MS : FOCUS_TTL_MS
      return now - record.time <= ttl
    })
    .slice(0, LIMIT)
}

/** 清理过期记录；有变化时落盘 */
export function pruneExpiredRecords() {
  if (!loaded.value) return
  const before = records.value.length
  records.value = pruneByRetention(records.value)
  if (records.value.length !== before) save()
}

const unreadCount = computed(() => records.value.filter(r => !r.read).length)

const KIND_LABELS: Record<
  'focus-done' | 'break-done' | 'task-reminder',
  { title: string; detail: string }
> = {
  'focus-done': { title: '专注完成', detail: '本轮番茄结束，休息一下' },
  'break-done': { title: '休息结束', detail: '准备好开始下一轮专注了' },
  'task-reminder': { title: '每日任务提醒', detail: '' },
}

async function load() {
  let prunedOnLoad = false
  try {
    const raw = await readTextFile(NOTIF_FILE, { baseDir: BaseDirectory.AppData })
    const parsed = JSON.parse(raw) as NotificationRecord[]
    const list = Array.isArray(parsed) ? parsed : []
    const kept = pruneByRetention(list)
    prunedOnLoad = kept.length !== list.length
    records.value = kept
  } catch {
    records.value = []
  }
  loaded.value = true
  // 启动时把过期记录同步落盘，避免文件里长期残留
  if (prunedOnLoad) save()
}

async function save() {
  if (!loaded.value) return
  try {
    await mkdir('focus-island', { baseDir: BaseDirectory.AppData, recursive: true })
    await writeTextFile(NOTIF_FILE, JSON.stringify(records.value), { baseDir: BaseDirectory.AppData })
  } catch (e) {
    console.error('Failed to save notifications', e)
  }
}

export function addRecord(kind: 'focus-done' | 'break-done' | 'task-reminder', detail?: string) {
  const labels = KIND_LABELS[kind]
  records.value = pruneByRetention([
    {
      id: `n-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`,
      kind,
      title: labels.title,
      detail: detail || labels.detail,
      time: Date.now(),
      read: false,
    },
    ...records.value,
  ])
  save()
}

function addSystemRecord(p: SystemNotificationPayload) {
  if (!shouldMirrorSystemNotification(p.app, p.aumid)) return
  const key = `${p.id}-${p.created_ms}`
  if (systemKeys.has(key)) return
  systemKeys.add(key)

  const appName = (p.app || '').trim()
  const title = (p.title || '').trim() || appName || '系统通知'
  const body = (p.body || '').trim()
  const detail = body || (appName && appName !== title ? appName : '来自系统通知中心')

  const record: NotificationRecord = {
    id: `sys-${key}`,
    kind: 'system',
    title,
    detail,
    time: p.created_ms || Date.now(),
    read: false,
    app: appName || undefined,
    aumid: p.aumid || undefined,
  }
  records.value = pruneByRetention([record, ...records.value])
  save()
}

function markAllRead() {
  records.value = records.value.map(r => (r.read ? r : { ...r, read: true }))
  save()
}

function clearAll() {
  records.value = []
  save()
}

export function formatNotifTime(ts: number): string {
  const d = new Date(ts)
  const now = new Date()
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
  if (d.toDateString() === now.toDateString()) return hm
  return `${d.getMonth() + 1}/${d.getDate()} ${hm}`
}

export function useNotifications() {
  if (!loaded.value) load()
  if (!listening) {
    listening = true
    // 面板端番茄钟推送的横幅事件，同步记录到通知中心
    void listen<'focus-done' | 'break-done' | null>('notification-show', ({ payload }) => {
      if (payload === 'focus-done' || payload === 'break-done') {
        addRecord(payload)
      }
    })
    // Windows 通知中心的系统通知镜像
    void listen<SystemNotificationPayload>('system-notification', ({ payload }) => {
      if (payload) addSystemRecord(payload)
    })
  }
  return { records, unreadCount, markAllRead, clearAll }
}
