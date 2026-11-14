import { ref, computed } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { readTextFile, writeTextFile, mkdir, BaseDirectory } from '@tauri-apps/plugin-fs'
import { shouldMirrorSystemNotification, rememberSystemNotificationSource, useSettings } from './useSettings'
import { useTimer } from './useTimer'

export type NotificationKind = 'focus-done' | 'break-done' | 'system' | 'task-reminder' | 'email'

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
  /** 每日任务提醒：对应任务 id，供“打开来源”跳转 */
  taskId?: string
  emailProvider?: string
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

/** Rust 端 email-notification 事件的载荷 */
export interface EmailNotificationPayload {
  uid: number
  from: string
  subject: string
  date: string
  provider: string
  mailbox_key: string
}

export function systemNotificationRecordId(p: SystemNotificationPayload) {
  return `sys-${p.id}-${p.created_ms}`
}

export function emailNotificationRecordId(p: EmailNotificationPayload) {
  return `email-${p.mailbox_key}-${p.uid}`
}

export function useExternalNotificationSilence() {
  const { settings } = useSettings()
  const timer = useTimer()
  return computed(() => settings.value.systemNotificationSilentInFocus
    && timer.phase.value === 'focus' && timer.running.value)
}

const NOTIF_FILE = 'focus-island/notifications.json'
const LIMIT = 200
// 系统通知按系统逻辑短期缓存；专注提醒保留更久
const SYSTEM_TTL_MS = 7 * 24 * 60 * 60 * 1000
const FOCUS_TTL_MS = 30 * 24 * 60 * 60 * 1000

const records = ref<NotificationRecord[]>([])
const loaded = ref(false)
let listening = false
let loading: Promise<void> | undefined
let pendingSave = Promise.resolve()

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

function save() {
  pendingSave = pendingSave.then(async () => {
    await mkdir('focus-island', { baseDir: BaseDirectory.AppData, recursive: true })
    await writeTextFile(NOTIF_FILE, JSON.stringify(records.value), { baseDir: BaseDirectory.AppData })
  }).catch(() => console.error('Failed to save notifications'))
}

function addAfterLoad(record: NotificationRecord) {
  const ready = loading ??= load()
  void ready.then(() => {
    if (records.value.some(item => item.id === record.id)) return
    records.value = pruneByRetention([record, ...records.value])
    save()
  })
}

export function addRecord(
  kind: 'focus-done' | 'break-done' | 'task-reminder',
  detail?: string,
  taskId?: string,
) {
  const labels = KIND_LABELS[kind]
  const id = `n-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`
  addAfterLoad({
    id,
    kind,
    title: labels.title,
    detail: detail || labels.detail,
    time: Date.now(),
    read: false,
    taskId: taskId || undefined,
  })
  return id
}

function addSystemRecord(p: SystemNotificationPayload) {
  const appName = (p.app || '').trim()
  const title = (p.title || '').trim() || appName || '系统通知'
  const body = (p.body || '').trim()
  const detail = body || (appName && appName !== title ? appName : '来自系统通知中心')

  const record: NotificationRecord = {
    id: systemNotificationRecordId(p),
    kind: 'system',
    title,
    detail,
    time: p.created_ms || Date.now(),
    read: false,
    app: appName || undefined,
    aumid: p.aumid || undefined,
  }
  addAfterLoad(record)
}

function addEmailRecord(p: EmailNotificationPayload) {
  const from = (p.from || '').trim() || '未知发件人'
  const record: NotificationRecord = {
    id: emailNotificationRecordId(p),
    kind: 'email',
    title: (p.subject || '').trim() || '(无主题)',
    detail: from,
    time: Date.now(),
    read: false,
    app: from,
    emailProvider: p.provider,
  }
  addAfterLoad(record)
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
  loading ??= load()
  if (!listening) {
    listening = true
    // 专注时静默只抑制岛内 peek，通知中心和邮件记录仍然保留。
    // 面板端番茄钟推送的横幅事件，同步记录到通知中心
    void listen<'focus-done' | 'break-done' | null>('notification-show', ({ payload }) => {
      if (payload === 'focus-done' || payload === 'break-done') {
        addRecord(payload)
      }
    })
    // Windows 通知中心的系统通知镜像
    void listen<SystemNotificationPayload>('system-notification', ({ payload }) => {
      if (!payload) return
      rememberSystemNotificationSource(payload.app, payload.aumid)
      if (shouldMirrorSystemNotification(payload.app, payload.aumid)) {
        addSystemRecord(payload)
      }
    })
    // 邮箱 IMAP 新邮件
    void listen<EmailNotificationPayload>('email-notification', ({ payload }) => {
      if (payload) addEmailRecord(payload)
    })
  }
  return { records, unreadCount, markAllRead, clearAll }
}
