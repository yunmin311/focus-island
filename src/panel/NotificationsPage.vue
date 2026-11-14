<script setup lang="ts">
import { computed, nextTick, onActivated, onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-shell'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import {
  useNotifications,
  formatNotifTime,
  pruneExpiredRecords,
  type NotificationRecord,
} from '../composables/useNotifications'
import {
  useSettings,
  isSystemNotificationSourceBlocked,
  systemNotificationSourceKey,
} from '../composables/useSettings'
import { useTasks } from '../composables/useTasks'
import { emailInboxUrl } from '../utils/emailLinks'

const props = defineProps<{ focusRequest?: { recordId?: string; openSource?: boolean; destination?: 'timer' } }>()
const emit = defineEmits<{ back: []; openTask: [taskId: string]; openHome: []; openTodo: [] }>()

const { records, unreadCount, markAllRead, clearAll } = useNotifications()
const { settings } = useSettings()
const { tasks } = useTasks()

// 系统通知与番茄钟/任务提醒分栏展示
const notifGroups = computed(() =>
  [
    { key: 'system', title: '系统通知', items: records.value.filter(r => r.kind === 'system') },
    { key: 'email', title: '邮件', items: records.value.filter(r => r.kind === 'email') },
    {
      key: 'focus',
      title: '专注提醒',
      items: records.value.filter(r => r.kind !== 'system' && r.kind !== 'email'),
    },
  ].filter(group => group.items.length > 0),
)

// 每组默认只展示前 7 条，其余折叠
const GROUP_VISIBLE_LIMIT = 7
const expandedGroups = ref<Record<string, boolean>>({})

function isGroupExpanded(key: string): boolean {
  return Boolean(expandedGroups.value[key])
}

function toggleGroup(key: string) {
  expandedGroups.value = {
    ...expandedGroups.value,
    [key]: !expandedGroups.value[key],
  }
}

function groupVisibleItems(group: { key: string; items: NotificationRecord[] }): NotificationRecord[] {
  return isGroupExpanded(group.key) ? group.items : group.items.slice(0, GROUP_VISIBLE_LIMIT)
}

// 单条通知：点击展开完整标题与正文，展开后可复制
const expandedRecords = ref<Record<string, boolean>>({})
const copiedId = ref<string | null>(null)
let copiedTimer: number | null = null

function isRecordExpanded(id: string): boolean {
  return Boolean(expandedRecords.value[id])
}

function toggleRecord(id: string) {
  expandedRecords.value = {
    ...expandedRecords.value,
    [id]: !expandedRecords.value[id],
  }
}

async function copyRecord(record: NotificationRecord) {
  try {
    const text = [record.app, record.title, record.detail].filter(Boolean).join('\n')
    await writeText(text)
    copiedId.value = record.id
    if (copiedTimer !== null) window.clearTimeout(copiedTimer)
    copiedTimer = window.setTimeout(() => {
      copiedId.value = null
      copiedTimer = null
    }, 1500)
  } catch {
    showActionStatus(record.id, '复制失败', false)
  }
}

// ---- 通知操作：打开来源 / 屏蔽来源 ----

const openingId = ref<string | null>(null)
const actionStatus = ref<{ id: string; message: string; ok: boolean } | null>(null)
let actionTimer: number | null = null
const focusedRecordId = ref<string | null>(null)
let focusTimer: number | null = null
const recordNodes = new Map<string, HTMLElement>()

function setRecordNode(id: string, el: unknown) {
  if (el instanceof HTMLElement) recordNodes.set(id, el)
  else recordNodes.delete(id)
}

function showActionStatus(id: string, message: string, ok: boolean) {
  actionStatus.value = { id, message, ok }
  if (actionTimer !== null) window.clearTimeout(actionTimer)
  actionTimer = ok ? window.setTimeout(() => {
    actionStatus.value = null
    actionTimer = null
  }, 2600) : null
}

function isRecordBlocked(record: NotificationRecord): boolean {
  if (record.kind !== 'system') return false
  return isSystemNotificationSourceBlocked(record.app ?? '', record.aumid ?? '')
}

function groupKeyForRecord(record: NotificationRecord): string {
  if (record.kind === 'system') return 'system'
  if (record.kind === 'email') return 'email'
  return 'focus'
}

function focusRecord(record: NotificationRecord) {
  expandedGroups.value = { ...expandedGroups.value, [groupKeyForRecord(record)]: true }
  expandedRecords.value = { ...expandedRecords.value, [record.id]: true }
  focusedRecordId.value = record.id
  if (focusTimer !== null) window.clearTimeout(focusTimer)
  focusTimer = window.setTimeout(() => {
    focusedRecordId.value = null
    focusTimer = null
  }, 1800)
  nextTick(() => {
    const node = recordNodes.get(record.id)
    node?.focus({ preventScroll: true })
    node?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  })
}

const pageHeading = ref<HTMLElement | null>(null)
let handledFocusRequest: typeof props.focusRequest
watch([() => props.focusRequest, records], ([request]) => {
  if (!request || handledFocusRequest === request) return
  if (request.destination === 'timer') {
    handledFocusRequest = request
    emit('openHome')
    return
  }
  const record = records.value.find(item => item.id === request.recordId)
  if (record) {
    handledFocusRequest = request
    if (request.openSource) void openRecordSource(record)
    else focusRecord(record)
  } else {
    void nextTick(() => pageHeading.value?.focus())
  }
}, { immediate: true, flush: 'post' })

async function openRecordSource(record: NotificationRecord) {
  if (openingId.value) return
  actionStatus.value = null

  if (record.kind === 'focus-done' || record.kind === 'break-done') {
    emit('openHome')
    return
  }
  if (record.kind === 'task-reminder') {
    const taskId = record.taskId
    if (taskId && tasks.value.some(task => task.id === taskId)) emit('openTask', taskId)
    else emit('openTodo')
    return
  }
  const inbox = emailInboxUrl(record.emailProvider)
  if (record.kind === 'email' && !inbox) {
    focusRecord(record)
    return
  }

  openingId.value = record.id
  try {
    if (record.kind === 'system') {
      await invoke('activate_notification_source', { app: record.app ?? '', aumid: record.aumid ?? '' })
      showActionStatus(record.id, '已打开来源应用', true)
    } else if (inbox) {
      await open(inbox)
    }
  } catch {
    focusRecord(record)
    const message = record.kind === 'system'
      ? '无法打开来源应用，已定位到这条通知'
      : '无法打开网页版收件箱，已定位到这条通知'
    showActionStatus(record.id, message, false)
  } finally {
    openingId.value = null
  }
}

function muteRecordSource(record: NotificationRecord) {
  const app = (record.app ?? '').trim()
  const aumid = (record.aumid ?? '').trim()
  const source = systemNotificationSourceKey(app, aumid)
  if (!source) {
    showActionStatus(record.id, '无法定位来源应用', false)
    return
  }

  const keys = [app.toLowerCase(), aumid.toLowerCase()].filter(Boolean)
  const matches = (entry: string) => keys.includes(entry.trim().toLowerCase())

  const muted = (settings.value.systemNotificationMutedApps ?? []).filter(entry => !matches(entry))
  settings.value.systemNotificationMutedApps = [...muted, source]
  settings.value.systemNotificationAllowedApps = (settings.value.systemNotificationAllowedApps ?? [])
    .filter(entry => !matches(entry))
  showActionStatus(record.id, '已屏蔽此应用，可在设置「通知来源」解除', true)
}

onUnmounted(() => {
  if (copiedTimer !== null) window.clearTimeout(copiedTimer)
  if (actionTimer !== null) window.clearTimeout(actionTimer)
  if (focusTimer !== null) window.clearTimeout(focusTimer)
})

// ---- 快捷设置：系统音量 ----

const sysVolume = ref(0)
const sysMuted = ref(false)
const volumeReady = ref(false)
const volumeError = ref<string | null>(null)
let applying = false

async function refreshVolume() {
  try {
    const state = await invoke<{ volume: number; muted: boolean }>('get_volume_state')
    sysVolume.value = Math.round(state.volume * 100)
    sysMuted.value = state.muted
    volumeReady.value = true
    volumeError.value = null
  } catch (e) {
    volumeError.value = typeof e === 'string' ? e : '无法读取系统音量'
  }
}

async function onVolumeInput(event: Event) {
  const v = Number((event.target as HTMLInputElement).value)
  sysVolume.value = v
  if (applying) return
  applying = true
  try {
    if (sysMuted.value && v > 0) {
      sysMuted.value = false
      await invoke('set_system_mute', { muted: false })
    }
    await invoke('set_system_volume', { volume: v / 100 })
  } catch {
    /* 滑动中失败不打断 */
  } finally {
    applying = false
  }
}

async function toggleMute() {
  const next = !sysMuted.value
  sysMuted.value = next
  try {
    await invoke('set_system_mute', { muted: next })
  } catch {
    sysMuted.value = !next
  }
}

onMounted(() => {
  refreshVolume()
  pruneExpiredRecords()
})
onActivated(pruneExpiredRecords)
</script>

<template>
  <div class="notif-page">
    <!-- 音量：紧凑工具条，不单独占一块卡片 -->
    <div v-if="volumeReady" class="volume-strip">
      <span class="volume-label">音量</span>
      <button class="mute-btn" :class="{ active: sysMuted }" @click="toggleMute" :title="sysMuted ? '取消静音' : '静音'">
        <svg v-if="sysMuted || sysVolume === 0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M11 5L6 9H2v6h4l5 4V5z" />
          <line x1="23" y1="9" x2="17" y2="15" />
          <line x1="17" y1="9" x2="23" y2="15" />
        </svg>
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M11 5L6 9H2v6h4l5 4V5z" />
          <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
          <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
        </svg>
      </button>
      <input
        type="range"
        class="volume-slider"
        min="0"
        max="100"
        :value="sysVolume"
        @input="onVolumeInput"
      />
      <span class="volume-value">{{ sysMuted ? '静音' : sysVolume }}</span>
    </div>
    <div v-else class="volume-hint">{{ volumeError ?? '读取系统音量中…' }}</div>

    <!-- 通知记录 -->
    <div class="section-head">
      <span ref="pageHeading" class="section-title" tabindex="-1">通知记录</span>
      <div class="record-tools">
        <button v-if="unreadCount" class="text-btn" @click="markAllRead">全部已读 ({{ unreadCount }})</button>
        <button v-if="records.length" class="text-btn" @click="clearAll">清空</button>
      </div>
    </div>
    <div v-if="records.length" class="notif-scroll">
      <section v-for="group in notifGroups" :key="group.key" class="notif-group">
        <div class="group-head">
          <span class="group-title">{{ group.title }}</span>
          <span class="group-count">{{ group.items.length }}</span>
        </div>
        <div class="notif-list">
          <div
            v-for="r in groupVisibleItems(group)"
            :key="r.id"
            :ref="el => setRecordNode(r.id, el)"
            class="notif-item"
            tabindex="-1"
            :class="{
              unread: !r.read,
              expanded: isRecordExpanded(r.id),
              focused: focusedRecordId === r.id,
            }"
            title="点击展开/收起"
            @click="toggleRecord(r.id)"
          >
            <div class="notif-dot" />
            <div class="notif-body">
              <div class="notif-line">
                <span
                  v-if="((r.kind === 'system' || r.kind === 'email') && r.app) || (r.kind === 'system' && isRecordBlocked(r))"
                  class="notif-app"
                >
                  <span v-if="r.app">{{ r.app }}</span>
                  <span v-if="r.kind === 'system' && isRecordBlocked(r)" class="notif-blocked">已屏蔽</span>
                </span>
                <span class="notif-title">{{ r.title }}</span>
                <span class="notif-time">{{ formatNotifTime(r.time) }}</span>
              </div>
              <div class="notif-detail">{{ r.detail }}</div>
              <div v-if="isRecordExpanded(r.id)" class="notif-actions" @click.stop>
                <button type="button" class="copy-btn" @click.stop="copyRecord(r)">
                  {{ copiedId === r.id ? '已复制' : '复制' }}
                </button>
                <button
                  type="button"
                  class="copy-btn"
                  :disabled="openingId === r.id"
                  @click.stop="openRecordSource(r)"
                >
                  {{ openingId === r.id ? '打开中…' : '打开来源' }}
                </button>
                <button
                  v-if="r.kind === 'system'"
                  type="button"
                  class="copy-btn"
                  :disabled="isRecordBlocked(r) || !systemNotificationSourceKey(r.app, r.aumid)"
                  @click.stop="muteRecordSource(r)"
                >
                  {{ isRecordBlocked(r) ? '已屏蔽' : '屏蔽来源' }}
                </button>
              </div>
              <div
                v-if="actionStatus && actionStatus.id === r.id"
                class="notif-action-status"
                :class="{ error: !actionStatus.ok }"
                role="status"
                aria-live="polite"
                @click.stop
              >
                {{ actionStatus.message }}
              </div>
            </div>
          </div>
        </div>
        <button
          v-if="group.items.length > GROUP_VISIBLE_LIMIT"
          type="button"
          class="group-more"
          @click="toggleGroup(group.key)"
        >
          {{ isGroupExpanded(group.key) ? '收起' : `展开剩余 ${group.items.length - GROUP_VISIBLE_LIMIT} 条` }}
        </button>
      </section>
    </div>
    <div v-else class="empty">还没有通知，完成番茄或收到系统通知后会记录在这里</div>
  </div>
</template>

<style scoped>
@reference "../styles.css";

.notif-page {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  box-sizing: border-box;
  overflow: hidden;
  padding: 0 12px 12px;
}

.page-topbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 0;
}

.back-btn {
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  border: 1px solid var(--v2-line);
  border-radius: 8px;
  background: var(--ui-well);
  color: var(--v2-ink);
  cursor: pointer;
}

.back-btn svg {
  width: 14px;
  height: 14px;
}

.back-btn:hover {
  background: var(--ui-hover);
}

.page-title {
  flex: 1;
  font-size: 14px;
  font-weight: 600;
  color: var(--v2-ink);
}

.text-btn {
  border: none;
  background: transparent;
  color: var(--v2-secondary);
  font-size: 11px;
  cursor: pointer;
  padding: 3px 5px;
  border-radius: 5px;
}

.text-btn:hover {
  color: var(--v2-ink);
  background: var(--ui-well);
}

.text-btn.placeholder {
  cursor: default;
}

.volume-strip {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 2px 8px;
  margin-bottom: 4px;
  border-bottom: 1px solid var(--v2-line);
  flex-shrink: 0;
}

.volume-label {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--v2-muted);
}

.mute-btn {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--v2-muted);
  cursor: pointer;
  flex-shrink: 0;
}

.mute-btn svg {
  width: 13px;
  height: 13px;
}

.mute-btn:hover {
  color: var(--v2-ink);
  background: var(--ui-well);
}

.mute-btn.active {
  color: var(--v2-accent);
  background: color-mix(in srgb, var(--v2-accent) 12%, transparent);
}

.volume-slider {
  flex: 1;
  accent-color: var(--focus-color);
}

.volume-value {
  min-width: 26px;
  text-align: right;
  font-size: 11px;
  color: var(--v2-secondary);
  font-variant-numeric: tabular-nums;
}

.volume-hint {
  font-size: 10.5px;
  color: var(--v2-muted);
}

.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 4px;
}

.section-head .section-title {
  margin-bottom: 0;
}

.notif-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding-right: 4px;
}

.notif-scroll::-webkit-scrollbar {
  width: 4px;
}

.notif-scroll::-webkit-scrollbar-track {
  background: transparent;
}

.notif-scroll::-webkit-scrollbar-thumb {
  background: var(--ui-hover);
  border-radius: 2px;
}

.notif-group {
  display: flex;
  flex-direction: column;
}

.group-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 0 2px 4px;
}

.group-title {
  font-size: 11.5px;
  font-weight: 600;
  color: var(--v2-secondary);
  letter-spacing: 0.3px;
}

.group-count {
  font-size: 10px;
  color: var(--v2-muted);
  background: var(--ui-well);
  padding: 0 5px;
  border-radius: 999px;
  font-variant-numeric: tabular-nums;
}

.notif-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.group-more {
  width: 100%;
  margin-top: 4px;
  padding: 6px;
  border-radius: 7px;
  background: var(--ui-well);
  border: 1px dashed var(--v2-line);;
  color: var(--v2-muted);
  font-size: 10.5px;
  font-family: inherit;
  cursor: pointer;
  transition: all 0.2s ease;
}

.group-more:hover {
  background: var(--ui-well);
  border-color: var(--v2-muted);
  color: var(--v2-ink);
}

.notif-item {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  box-sizing: border-box;
  min-width: 0;
  padding: 9px 11px;
  border-radius: 8px;
  border: 1px solid var(--v2-line);;
  background: var(--ui-well);
  cursor: pointer;
  transition: background 0.15s ease, border-color 0.15s ease;
}

.notif-item:hover {
  background: var(--ui-well);
  border-color: var(--v2-muted);
}

.notif-item.unread {
  border-color: var(--v2-muted);
  background: var(--ui-well);
}

.notif-item.focused {
  border-color: color-mix(in srgb, var(--focus-color) 52%, transparent);
  background: color-mix(in srgb, var(--focus-color) 9%, transparent);
}

.notif-dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  margin-top: 5px;
  background: var(--ui-hover);
  flex-shrink: 0;
}

.notif-item.unread .notif-dot {
  background: var(--focus-color);
}

.notif-body {
  flex: 1;
  min-width: 0;
}

.notif-line {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.notif-title {
  flex: 1;
  min-width: 0;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--v2-ink);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.notif-app {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
  max-width: 40%;
  font-size: 10px;
  color: var(--v2-muted);
  background: var(--ui-well);
  padding: 2px 6px;
  border-radius: 4px;
  line-height: 1.4;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.notif-blocked {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--v2-muted);
  border: 0;
  border-left: 1px solid var(--v2-line);;
  padding: 0 0 0 4px;
  line-height: 1.4;
  white-space: nowrap;
}

.notif-time {
  font-size: 10px;
  color: var(--v2-muted);
  font-variant-numeric: tabular-nums;
}

.notif-detail {
  font-size: 11px;
  line-height: 1.4;
  color: var(--v2-muted);
  margin-top: 2px;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  overflow-wrap: anywhere;
  word-break: break-word;
}

.notif-item.expanded .notif-title {
  white-space: normal;
  overflow: visible;
  text-overflow: clip;
  overflow-wrap: anywhere;
}

.notif-item.expanded .notif-detail {
  display: block;
  overflow: visible;
  -webkit-line-clamp: unset;
  overflow-wrap: anywhere;
  word-break: break-word;
}

.notif-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 6px;
  margin-top: 6px;
}

.copy-btn {
  flex-shrink: 0;
  border: none;
  background: var(--ui-well);
  color: var(--v2-secondary);
  font-size: 10.5px;
  font-family: inherit;
  padding: 3px 9px;
  border-radius: 5px;
  white-space: nowrap;
  cursor: pointer;
  transition: all 0.15s ease;
}

.copy-btn:hover {
  background: var(--ui-hover);
  color: var(--v2-ink);
}

.copy-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.copy-btn:disabled:hover {
  background: var(--ui-well);
  color: var(--v2-secondary);
}

.notif-action-status {
  margin-top: 4px;
  font-size: 10.5px;
  line-height: 1.4;
  color: var(--v2-secondary);
  overflow-wrap: anywhere;
}

.notif-action-status.error {
  color: var(--alert-color);
}

.empty {
  flex: 1;
  display: grid;
  place-items: center;
  font-size: 12.5px;
  color: var(--v2-muted);
}
</style>
