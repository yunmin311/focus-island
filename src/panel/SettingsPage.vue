<script setup lang="ts">
import { computed, ref, watch, nextTick, onMounted, onUnmounted, onActivated, onDeactivated } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettings, presetThemes, type ColorMode, type IslandDisplayMode, type EmailProvider } from '../composables/useSettings'
import { useNotifications } from '../composables/useNotifications'
import { useUpdater } from '../composables/useUpdater'
import { useForkUpdate } from '../composables/useForkUpdate'
import AboutDialog from './AboutDialog.vue'
import BackButton from './BackButton.vue'

const emit = defineEmits<{ back: [] }>()

// 定制构建：应用内更新指向官方版会覆盖定制内容，已禁用
const SHOW_UPDATER = false

const { settings, resetSettings } = useSettings()
const { records } = useNotifications()

// 系统通知镜像：来源屏蔽（最近来源 + 已屏蔽来源合并展示，确保可撤销）
const mirrorSources = computed(() => {
  const keys: string[] = []
  const push = (value: string) => {
    const key = value.trim()
    if (key && !keys.includes(key)) keys.push(key)
  }
  for (const key of settings.value.systemNotificationKnownSources ?? []) push(key)
  for (const record of records.value) {
    if (record.kind === 'system') push(record.app || record.aumid || '')
  }
  for (const key of settings.value.systemNotificationMutedApps ?? []) push(key)
  for (const key of settings.value.systemNotificationAllowedApps ?? []) push(key)
  return keys.slice(0, 60)
})

const sourcesExpanded = ref(false)
const sourcesProbing = ref(false)
const sourcesStatus = ref('')

const sourceSummary = computed(() => {
  const total = mirrorSources.value.length
  if (total === 0) return '暂无来源'
  if ((settings.value.systemNotificationMirrorMode ?? 'all') === 'selected') {
    const allowed = (settings.value.systemNotificationAllowedApps ?? []).length
    return `${total} 个来源 · ${allowed} 个已允许`
  }
  const muted = (settings.value.systemNotificationMutedApps ?? []).length
  return `${total} 个来源 · ${muted} 个已屏蔽`
})

async function probeNotificationSources() {
  sourcesProbing.value = true
  sourcesStatus.value = '正在检测...'
  try {
    const sources = await invoke<Array<{ app: string; aumid: string }>>('probe_notification_sources')
    const names = sources.map(source => (source.app || source.aumid || '').trim()).filter(Boolean)
    const existing = settings.value.systemNotificationKnownSources ?? []
    const added = names.filter(
      name => !existing.some(entry => entry.toLowerCase() === name.toLowerCase()),
    )
    const merged = [...existing]
    for (const name of names) {
      if (!merged.some(entry => entry.toLowerCase() === name.toLowerCase())) {
        merged.push(name)
      }
    }
    settings.value.systemNotificationKnownSources = merged.slice(-80)
    sourcesStatus.value = names.length
      ? `检测到 ${names.length} 个来源${added.length ? `，新增 ${added.length} 个` : ''}`
      : '当前通知中心没有可检测的来源'
  } catch (e) {
    sourcesStatus.value = typeof e === 'string' ? e : '检测来源失败'
  } finally {
    sourcesProbing.value = false
  }
}

function isSourceMuted(source: string): boolean {
  const list = settings.value.systemNotificationMutedApps ?? []
  return list.some(key => key.toLowerCase() === source.toLowerCase())
}

function isSourceAllowed(source: string): boolean {
  const list = settings.value.systemNotificationAllowedApps ?? []
  return list.some(key => key.toLowerCase() === source.toLowerCase())
}

function toggleSourceMute(source: string) {
  const list = settings.value.systemNotificationMutedApps ?? []
  settings.value.systemNotificationMutedApps = isSourceMuted(source)
    ? list.filter(key => key.toLowerCase() !== source.toLowerCase())
    : [...list, source]
}

function toggleSourceAllowed(source: string) {
  const list = settings.value.systemNotificationAllowedApps ?? []
  settings.value.systemNotificationAllowedApps = isSourceAllowed(source)
    ? list.filter(key => key.toLowerCase() !== source.toLowerCase())
    : [...list, source]
}

function toggleSource(source: string) {
  if ((settings.value.systemNotificationMirrorMode ?? 'all') === 'selected') {
    toggleSourceAllowed(source)
  } else {
    toggleSourceMute(source)
  }
}

// ---- 邮箱通知 ----

const emailProviders: Array<{ id: EmailProvider; label: string }> = [
  { id: 'qq', label: 'QQ' },
  { id: '163', label: '163' },
  { id: '126', label: '126' },
  { id: 'gmail', label: 'Gmail' },
  { id: 'outlook', label: 'Outlook' },
  { id: 'custom', label: '自定义' },
]
const emailAuthCode = ref('')
const emailSaving = ref(false)
const emailTesting = ref(false)
const emailHasCredential = ref(false)
const emailStatus = ref('')
const emailStatusOk = ref<boolean | null>(null)
const emailExpanded = ref(false)

const emailProviderLabel = computed(
  () => emailProviders.find(provider => provider.id === settings.value.emailProvider)?.label ?? 'QQ',
)

const emailSummary = computed(() => {
  const address = settings.value.emailAddress.trim()
  if (!address) return '未配置'
  const credential = emailHasCredential.value ? '已保存授权码' : '未保存授权码'
  if (settings.value.emailProvider === 'custom') {
    const host = settings.value.emailImapHost.trim() || '未填写服务器'
    return `${host} · ${address} · ${credential}`
  }
  return `${emailProviderLabel.value} · ${address} · ${credential}`
})

const emailAuthPlaceholder = computed(() => {
  switch (settings.value.emailProvider) {
    case 'gmail':
      return 'Gmail 应用专用密码'
    case 'outlook':
      return 'Outlook 应用专用密码'
    case 'custom':
      return '授权码 / 应用专用密码'
    default:
      return '授权码（不是登录密码）'
  }
})

watch(
  () => settings.value.emailEnabled,
  (enabled) => {
    if (enabled && !settings.value.emailAddress.trim()) {
      emailExpanded.value = true
    }
  },
)

async function refreshEmailCredentialStatus() {
  const email = settings.value.emailAddress.trim()
  if (!email) {
    emailHasCredential.value = false
    return
  }
  try {
    emailHasCredential.value = await invoke<boolean>('email_credential_status', { email })
  } catch {
    emailHasCredential.value = false
  }
}

async function saveEmailCredential() {
  const email = settings.value.emailAddress.trim()
  const code = emailAuthCode.value.trim()
  if (!email || !code) {
    emailStatus.value = '请填写邮箱地址和授权码'
    emailStatusOk.value = false
    return
  }
  emailSaving.value = true
  try {
    await invoke('save_email_credential', { email, password: code })
    emailAuthCode.value = ''
    emailHasCredential.value = true
    emailStatus.value = '授权码已保存到 Windows 凭据管理器'
    emailStatusOk.value = true
  } catch (e) {
    emailStatus.value = typeof e === 'string' ? e : '保存授权码失败'
    emailStatusOk.value = false
  } finally {
    emailSaving.value = false
  }
}

async function testEmailConnection() {
  const email = settings.value.emailAddress.trim()
  if (!email) {
    emailStatus.value = '请先填写邮箱地址'
    emailStatusOk.value = false
    return
  }
  if (settings.value.emailProvider === 'custom' && !settings.value.emailImapHost.trim()) {
    emailStatus.value = '请填写 IMAP 服务器'
    emailStatusOk.value = false
    return
  }
  emailTesting.value = true
  emailStatus.value = '正在连接...'
  emailStatusOk.value = null
  try {
    const message = await invoke<string>('test_email_connection', {
      provider: settings.value.emailProvider,
      email,
      host: settings.value.emailProvider === 'custom' ? settings.value.emailImapHost.trim() : '',
    })
    emailStatus.value = message
    emailStatusOk.value = true
  } catch (e) {
    emailStatus.value = typeof e === 'string' ? e : '连接失败'
    emailStatusOk.value = false
  } finally {
    emailTesting.value = false
  }
}

async function clearEmailCredential() {
  try {
    await invoke('delete_email_credential')
    emailHasCredential.value = false
    emailStatus.value = '已清除本机保存的授权码'
    emailStatusOk.value = null
  } catch (e) {
    emailStatus.value = typeof e === 'string' ? e : '清除失败'
    emailStatusOk.value = false
  }
}

function resetAllSettings() {
  if (!window.confirm('恢复所有设置为默认？主题、透明度、显示模式、快捷键和标语都会重置。')) return
  resetSettings()
}

// YM 版更新：只检查自己的 Release，点击后跳转下载
const {
  checking: forkChecking,
  available: forkUpdateAvailable,
  statusText: forkUpdateStatus,
  check: checkForkUpdate,
  openLatest: openLatestRelease,
} = useForkUpdate()

// 标语编辑：每行一条，清空则恢复内置默认
const mottoText = computed({
    get: () => (settings.value.idleMottos && settings.value.idleMottos.length ? settings.value.idleMottos.join('\n') : ''),
    set: (v: string) => {
        settings.value.idleMottos = v.split('\n').map(line => line.trim()).filter(Boolean)
    },
})
const {
  checking,
  checked,
  updateAvailable,
  downloading,
  installing,
  downloadProgress,
  error,
  updateInfo,
  installFinished,
  installFailed,
  downloadFailed,
  checkForUpdate,
  downloadAndInstall,
  openReleasePage,
} = useUpdater()

const opacityPercent = (v: number) => Math.round(v * 100) + '%'

const showAbout = ref(false)
const isRecording = ref(false)

const colorModeOptions: Array<{ id: ColorMode; label: string }> = [
  { id: 'dark', label: '深色' },
  { id: 'light', label: '浅色' },
  { id: 'system', label: '跟随系统' },
]

const displayModeOptions: Array<{ id: IslandDisplayMode; label: string; hint: string }> = [
  { id: 'resident', label: '常驻', hint: '胶囊始终显示在屏幕顶部' },
  { id: 'dock', label: '边缘停靠', hint: '贴边收起为专注条，鼠标悬停时展开' },
  { id: 'hidden', label: '完全隐藏', hint: '隐藏灵动岛与刘海，仅通过快捷键或托盘打开面板' },
]

const updateStatusText = computed(() => {
  if (downloading.value) return `下载中 ${downloadProgress.value}%`
  if (installing.value) return '安装中，请稍候...'
  if (updateAvailable.value) return `发现新版本 v${updateInfo.value?.version ?? ''}`
  if (installFinished.value) return '更新已安装，正在准备重启'
  if (error.value) return error.value
  if (checked.value) return '已是最新版本'
  return '自动检查开启后，启动时静默检查，有新版本会在面板顶部提示'
})

const showManualUpdateDownload = computed(() => installFailed.value && Boolean(error.value))
const updateFailureLabel = computed(() => downloadFailed.value ? '下载失败' : '安装失败')

function formatShortcut(key: string): string {
  if (!key) return '无'
  return key.replace('CommandOrControl', navigator.userAgent.includes('Mac') ? '⌘' : 'Ctrl')
    .replace('Alt', 'Alt')
    .replace('Shift', '⇧')
    .replace('Control', 'Ctrl')
}

function startRecording() {
  isRecording.value = true
}

function stopRecording() {
  isRecording.value = false
}

let recordedKey = ''

function buildCombo(e: KeyboardEvent): string {
  const mods: string[] = []
  if (e.ctrlKey) mods.push('Control')
  if (e.altKey) mods.push('Alt')
  if (e.metaKey) mods.push('CommandOrControl')
  if (e.shiftKey) mods.push('Shift')

  const code = e.code
  let keyName: string
  if (code.startsWith('Key')) keyName = code.slice(3)
  else if (code.startsWith('Digit')) keyName = code.slice(5)
  else keyName = code

  return mods.join('+') + '+' + keyName
}

function onRecordKeydown(e: KeyboardEvent) {
  if (!isRecording.value) return
  // Skip modifier-only presses
  if (e.key === 'Control' || e.key === 'Alt' || e.key === 'Meta' || e.key === 'Shift') return
  // Skip if no modifier is held
  if (!e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) return

  e.preventDefault()
  e.stopPropagation()
  recordedKey = buildCombo(e)
}

function onRecordKeyup(e: KeyboardEvent) {
  if (!isRecording.value) return

  // Fallback: if keydown was intercepted (e.g. Option+Space on macOS),
  // capture the combo from keyup instead
  if (!recordedKey && (e.ctrlKey || e.altKey || e.metaKey || e.shiftKey)) {
    if (e.key !== 'Control' && e.key !== 'Alt' && e.key !== 'Meta' && e.key !== 'Shift') {
      recordedKey = buildCombo(e)
    }
  }

  // Finalize when all modifiers are released
  if (!e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
    if (recordedKey) {
      settings.value.shortcutKey = recordedKey
      recordedKey = ''
      isRecording.value = false
    } else {
      // Released all mods without hitting a key — cancel
      isRecording.value = false
    }
  }
}

function resetRecording() {
  stopRecording()
  recordedKey = ''
}

function goBack() {
  showAbout.value = false
  resetRecording()
  nextTick()
  emit('back')
}

function switchColorMode(mode: ColorMode) {
  if (settings.value.colorMode === mode) return
  settings.value.colorMode = mode
}

function manualCheckForUpdate() {
  void checkForUpdate()
}

function installUpdate() {
  void downloadAndInstall()
}

function openManualDownload() {
  openReleasePage()
}

let recordListenersBound = false

function bindRecordListeners() {
  if (recordListenersBound) return
  document.addEventListener('keydown', onRecordKeydown, true)
  document.addEventListener('keyup', onRecordKeyup, true)
  recordListenersBound = true
}

function unbindRecordListeners() {
  if (!recordListenersBound) return
  document.removeEventListener('keydown', onRecordKeydown, true)
  document.removeEventListener('keyup', onRecordKeyup, true)
  recordListenersBound = false
}

onMounted(() => {
  bindRecordListeners()
  void refreshEmailCredentialStatus()
  if (settings.value.autoCheckUpdates) {
    void checkForkUpdate(false)
  }
})

onActivated(() => {
  bindRecordListeners()
  void refreshEmailCredentialStatus()
})

onDeactivated(() => {
  resetRecording()
  unbindRecordListeners()
})

onUnmounted(() => {
  unbindRecordListeners()
})
</script>

<template>
  <div class="settings-page" :class="{ 'blurred': showAbout }">
    <div class="settings-header">
      <BackButton @click="goBack" />
      <button class="about-btn" @click="showAbout = true">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
          <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-6h2v6zm0-8h-2V7h2v2z"/>
        </svg>
      </button>
    </div>

    <div class="settings-section">
      <div class="section-title">显示模式</div>
      <div class="mode-toggle" role="group" aria-label="主题模式">
        <button
          v-for="option in colorModeOptions"
          :key="option.id"
          type="button"
          class="mode-btn"
          :class="{ active: settings.colorMode === option.id }"
          @click="switchColorMode(option.id)"
        >
          {{ option.label }}
        </button>
      </div>

      <div class="section-subtitle">显示方式</div>
      <div class="mode-toggle" role="group" aria-label="灵动岛显示方式">
        <button
          v-for="opt in displayModeOptions"
          :key="opt.id"
          type="button"
          class="mode-btn"
          :class="{ active: settings.islandDisplayMode === opt.id }"
          @click="settings.islandDisplayMode = opt.id"
          :title="opt.hint"
        >
          {{ opt.label }}
        </button>
      </div>
    </div>

    <!-- 更新 -->
    <div class="settings-section">
      <div class="section-title">交互</div>
      <div class="update-toggle-row">
        <div class="update-copy">
          <span class="update-label">点击刘海打开面板</span>
          <span class="update-hint">关闭后仅通过快捷键或托盘图标打开面板</span>
        </div>
        <label class="switch" aria-label="点击刘海打开面板">
          <input v-model="settings.notchClickOpensPanel" type="checkbox" />
          <span class="switch-track"></span>
        </label>
      </div>
    </div>
    <div class="settings-section">
      <div class="section-title">声音与启动</div>
      <div class="update-toggle-row">
        <div class="update-copy">
          <span class="update-label">通知提示音</span>
          <span class="update-hint">番茄钟或休息结束时播放提示音</span>
        </div>
        <label class="switch" aria-label="通知提示音">
          <input v-model="settings.notificationSound" type="checkbox" />
          <span class="switch-track"></span>
        </label>
      </div>
      <div class="update-toggle-row" style="margin-top: 8px;">
        <div class="update-copy">
          <span class="update-label">开机自动启动</span>
          <span class="update-hint">系统启动时自动运行 Focus Island</span>
        </div>
        <label class="switch" aria-label="开机自动启动">
          <input v-model="settings.autoLaunch" type="checkbox" />
          <span class="switch-track"></span>
        </label>
      </div>
      <div v-if="SHOW_UPDATER" class="update-toggle-row" style="margin-top: 8px;">
        <div class="update-copy">
          <span class="update-label">自动检查更新</span>
          <span class="update-hint">启动后静默检查，不打断专注</span>
        </div>
        <label class="switch" aria-label="自动检查更新">
          <input v-model="settings.autoCheckUpdates" type="checkbox" />
          <span class="switch-track"></span>
        </label>
      </div>
      <div v-if="SHOW_UPDATER" class="update-action-row">
        <button
          type="button"
          class="update-btn"
          :disabled="checking || downloading || installing"
          @click="manualCheckForUpdate"
        >
          <span v-if="checking" class="mini-spinner"></span>
          <span>{{ checking ? '检查中...' : '手动检查更新' }}</span>
        </button>
        <button
          v-if="updateAvailable"
          type="button"
          class="update-btn primary"
          :disabled="downloading || installing"
          @click="installUpdate"
        >
          {{ installing ? '安装中...' : '下载并安装更新' }}
        </button>
        <span
          class="update-status"
          :class="{ error: Boolean(error), success: checked && !updateAvailable && !error }"
        >
          {{ updateStatusText }}
        </span>
      </div>
      <div v-if="showManualUpdateDownload" class="update-recovery">
        <span>{{ updateFailureLabel }}，可以从 GitHub Release 手动下载安装包。</span>
        <button type="button" class="update-link-btn" @click="openManualDownload">
          前往 GitHub Release
        </button>
      </div>
    </div>

    <!-- 系统通知镜像 -->
    <div class="settings-section">
      <div class="section-title">系统通知</div>
      <div class="update-toggle-row">
        <div class="update-copy">
          <span class="update-label">镜像 Windows 通知</span>
          <span class="update-hint">记录到通知中心，并在岛上短暂提示；系统原生通知不受影响</span>
        </div>
        <label class="switch" aria-label="镜像 Windows 通知">
          <input v-model="settings.systemNotificationMirror" type="checkbox" />
          <span class="switch-track"></span>
        </label>
      </div>
      <div v-if="settings.systemNotificationMirror" class="setting-option-row">
        <span class="update-label">镜像范围</span>
        <div class="mode-toggle">
          <button
            type="button"
            class="mode-btn"
            :class="{ active: (settings.systemNotificationMirrorMode ?? 'all') === 'all' }"
            @click="settings.systemNotificationMirrorMode = 'all'"
          >
            全部
          </button>
          <button
            type="button"
            class="mode-btn"
            :class="{ active: settings.systemNotificationMirrorMode === 'selected' }"
            @click="settings.systemNotificationMirrorMode = 'selected'"
          >
            仅选中
          </button>
        </div>
      </div>
      <button
        v-if="settings.systemNotificationMirror"
        type="button"
        class="collapsible-row row-spaced"
        @click="sourcesExpanded = !sourcesExpanded"
      >
        <span class="update-copy">
          <span class="update-label">通知来源</span>
          <span class="update-hint">{{ sourceSummary }}</span>
        </span>
        <svg
          class="collapsible-chevron"
          :class="{ open: sourcesExpanded }"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
      <div v-if="settings.systemNotificationMirror && sourcesExpanded" class="mirror-sources">
        <div class="mirror-sources-head">
          <span class="update-hint">
            {{ (settings.systemNotificationMirrorMode ?? 'all') === 'all' ? '点按来源可屏蔽' : '点按来源加入允许列表' }}
          </span>
          <button
            type="button"
            class="source-probe-btn"
            :disabled="sourcesProbing"
            @click="probeNotificationSources"
          >
            {{ sourcesProbing ? '检测中...' : '检测来源' }}
          </button>
        </div>
        <span v-if="sourcesStatus" class="source-status">{{ sourcesStatus }}</span>
        <span
          v-if="(settings.systemNotificationMirrorMode ?? 'all') === 'selected' && !(settings.systemNotificationAllowedApps ?? []).length"
          class="mirror-empty"
        >
          允许列表为空，当前不会镜像任何通知
        </span>
        <div class="mirror-chips">
          <button
            v-for="source in mirrorSources"
            :key="source"
            type="button"
            class="mirror-chip"
            :class="{
              muted: (settings.systemNotificationMirrorMode ?? 'all') === 'all' && isSourceMuted(source),
              selected: settings.systemNotificationMirrorMode === 'selected' && isSourceAllowed(source),
            }"
            @click="toggleSource(source)"
          >
            {{ source }}
          </button>
        </div>
        <span v-if="!mirrorSources.length" class="mirror-empty">
          还没有来源，收到通知或点「检测来源」后会出现
        </span>
      </div>
      <div class="update-toggle-row row-spaced">
        <div class="update-copy">
          <span class="update-label">专注时静默</span>
          <span class="update-hint">番茄计时运行中，系统通知与邮件只记录到通知中心，不弹岛内提示</span>
        </div>
        <label class="switch" aria-label="专注时静默">
          <input v-model="settings.systemNotificationSilentInFocus" type="checkbox" />
          <span class="switch-track"></span>
        </label>
      </div>
    </div>

    <!-- 邮箱通知 -->
    <div class="settings-section">
      <div class="section-title">邮箱通知</div>
      <div class="update-toggle-row">
        <div class="update-copy">
          <span class="update-label">邮件提醒</span>
          <span class="update-hint">只取发件人、主题和时间；专注计时期间只记录、不弹提示</span>
        </div>
        <label class="switch" aria-label="邮件提醒">
          <input v-model="settings.emailEnabled" type="checkbox" />
          <span class="switch-track"></span>
        </label>
      </div>
      <button type="button" class="collapsible-row row-spaced" @click="emailExpanded = !emailExpanded">
        <span class="update-copy">
          <span class="update-label">账号与授权码</span>
          <span class="update-hint">{{ emailSummary }}</span>
        </span>
        <svg
          class="collapsible-chevron"
          :class="{ open: emailExpanded }"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
      <div v-if="emailExpanded" class="email-form">
        <select v-model="settings.emailProvider" class="email-input email-select">
          <option v-for="provider in emailProviders" :key="provider.id" :value="provider.id">
            {{ provider.label }}
          </option>
        </select>
        <input
          v-if="settings.emailProvider === 'custom'"
          v-model="settings.emailImapHost"
          class="email-input"
          placeholder="IMAP 服务器，例如 imap.example.com"
          autocomplete="off"
        />
        <input
          v-model="settings.emailAddress"
          class="email-input"
          type="email"
          placeholder="邮箱地址"
          autocomplete="off"
          @blur="refreshEmailCredentialStatus"
        />
        <input
          v-model="emailAuthCode"
          class="email-input"
          type="password"
          :placeholder="emailAuthPlaceholder"
          autocomplete="off"
        />
        <div class="email-actions">
          <button type="button" class="update-btn primary" :disabled="emailSaving" @click="saveEmailCredential">
            {{ emailSaving ? '保存中...' : '保存授权码' }}
          </button>
          <button
            type="button"
            class="update-btn"
            :disabled="emailTesting || !emailHasCredential"
            @click="testEmailConnection"
          >
            {{ emailTesting ? '测试中...' : '测试连接' }}
          </button>
          <button v-if="emailHasCredential" type="button" class="update-btn" @click="clearEmailCredential">
            清除
          </button>
        </div>
        <span class="email-status" :class="{ error: emailStatusOk === false, success: emailStatusOk === true }">
          {{ emailStatus || (emailHasCredential ? '授权码已保存在本机' : '尚未保存授权码') }}
        </span>
        <span class="email-hint">
          QQ / 163 / 126 需开启 IMAP 并使用授权码；Gmail / Outlook 使用应用专用密码（若可用）；其他邮箱选「自定义」并填写 IMAP 服务器。授权码只保存在本机 Windows 凭据管理器。
        </span>
      </div>
    </div>

    <!-- 更新（YM 版） -->
    <div class="settings-section">
      <div class="section-title">更新（YM 版）</div>
      <div class="setting-row">
        <div class="update-copy">
          <span class="update-label">从我的 Release 检查更新</span>
          <span class="update-hint">只查询 yunmin311/focus-island，发现新版跳转下载页</span>
          <span class="update-status" :class="{ error: Boolean(forkUpdateStatus.startsWith('检查失败')) }">
            {{ forkUpdateStatus }}
          </span>
        </div>
        <div class="setting-actions">
          <button
            v-if="forkUpdateAvailable"
            type="button"
            class="update-btn primary"
            @click="openLatestRelease"
          >
            前往下载
          </button>
          <button
            type="button"
            class="update-btn"
            :disabled="forkChecking"
            @click="checkForkUpdate(true)"
          >
            <span v-if="forkChecking" class="mini-spinner"></span>
            <span>{{ forkChecking ? '检查中...' : '检查更新' }}</span>
          </button>
        </div>
      </div>
    </div>

    <!-- 外观 -->
    <div class="settings-section">
      <div class="section-title">外观</div>
      <div class="slider-row">
        <label class="slider-label">灵动岛透明度</label>
        <input
          type="range"
          class="opacity-slider"
          min="0"
          max="0.9"
          step="0.01"
          v-model.number="settings.islandOpacity"
        />
        <span class="opacity-value">{{ opacityPercent(settings.islandOpacity) }}</span>
      </div>
      <div class="slider-row">
        <label class="slider-label">面板透明度</label>
        <input
          type="range"
          class="opacity-slider"
          min="0"
          max="0.9"
          step="0.01"
          v-model.number="settings.panelOpacity"
        />
        <span class="opacity-value">{{ opacityPercent(settings.panelOpacity) }}</span>
      </div>
      <div class="slider-row">
        <label class="slider-label">大小</label>
        <input
          type="range"
          class="opacity-slider"
          min="0.5"
          max="1.5"
          step="0.1"
          v-model.number="settings.islandScale"
        />
        <span class="opacity-value">{{ Math.round(settings.islandScale * 100) }}%</span>
      </div>
    </div>

    <!-- 标语 -->
    <div class="settings-section">
      <div class="section-title">标语</div>
      <textarea
        class="motto-editor"
        rows="4"
        placeholder="每行一条自定义标语，留空使用默认"
        v-model="mottoText"
      ></textarea>
      <div class="motto-hint">空闲时在灵动岛轮播显示；每行一条，清空恢复默认</div>
    </div>
    <div class="settings-section">
      <div class="section-title">番茄钟</div>
      <div class="duration-grid">
        <div class="duration-cell">
          <label class="duration-label">专注</label>
          <div class="stepper">
            <button class="stepper-btn" @click="settings.focusDuration = Math.max(5, Math.floor((settings.focusDuration - 1) / 5) * 5)">−</button>
            <span class="stepper-value">{{ settings.focusDuration }}</span>
            <button class="stepper-btn" @click="settings.focusDuration = Math.min(120, Math.ceil((settings.focusDuration + 1) / 5) * 5)">+</button>
          </div>
          <span class="duration-unit">分</span>
        </div>
        <div class="duration-cell">
          <label class="duration-label">休息</label>
          <div class="stepper">
            <button class="stepper-btn" @click="settings.breakDuration = Math.max(1, settings.breakDuration - 1)">−</button>
            <span class="stepper-value">{{ settings.breakDuration }}</span>
            <button class="stepper-btn" @click="settings.breakDuration = Math.min(30, settings.breakDuration + 1)">+</button>
          </div>
          <span class="duration-unit">分</span>
        </div>
      </div>
    </div>

    <!-- 快捷操作 -->
    <div class="settings-section">
      <div class="section-title">快捷操作</div>
      <div class="shortcut-row">
        <span class="shortcut-label">打开面板</span>
        <button
          class="shortcut-recorder"
          :class="{ recording: isRecording }"
          @click="startRecording"
          tabindex="0"
        >
          <span v-if="isRecording" class="recording-hint">按下组合键...</span>
          <span v-else>{{ formatShortcut(settings.shortcutKey) }}</span>
        </button>
        <span class="shortcut-hint" v-if="!isRecording">推荐: Ctrl+P、Alt+Space;默认Esc 关闭面板</span>
      </div>
    </div>

    <!-- 重置 -->
    <div class="settings-section">
      <div class="section-title">重置</div>
      <div class="setting-row">
        <div class="update-copy">
          <span class="update-label">恢复默认设置</span>
          <span class="update-hint">主题、透明度、显示模式、快捷键、标语全部恢复默认</span>
        </div>
        <div class="setting-actions">
          <button type="button" class="update-btn danger" @click="resetAllSettings">恢复默认设置</button>
        </div>
      </div>
    </div>

    <!-- 色彩主题 -->
    <div class="settings-section">
      <div class="section-title">色彩主题</div>
      <div class="theme-grid">
        <button
          v-for="theme in presetThemes"
          :key="theme.id"
          class="theme-card"
          :class="{ active: settings.activeThemeId === theme.id }"
          @click="settings.activeThemeId = theme.id"
        >
          <div class="theme-colors">
            <span class="color-dot" :style="{ background: theme.focusColor }"></span>
            <span class="color-dot" :style="{ background: theme.breakColor }"></span>
            <span class="color-dot" :style="{ background: theme.idleColor }"></span>
          </div>
          <span class="theme-name">{{ theme.name }}</span>
          <span v-if="settings.activeThemeId === theme.id" class="theme-check">✓</span>
        </button>
      </div>
    </div>
  </div>

  <!-- 关于弹窗 -->
  <AboutDialog v-if="showAbout" @close="showAbout = false" />
</template>

<style scoped>
@reference "../styles.css";

.settings-page {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  color: #e8e8ea;
}

.settings-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  flex-shrink: 0;
}

.back-btn {
  width: 28px;
  height: 28px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.7);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s;
}

.back-btn:hover {
  background: rgba(255, 255, 255, 0.12);
  border-color: rgba(255, 255, 255, 0.2);
  color: #fff;
}

.about-btn {
  width: 28px;
  height: 28px;
  border-radius: 6px;
  background: transparent;
  border: none;
  color: rgba(255, 255, 255, 0.5);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s;
}

.about-btn:hover {
  color: var(--focus-color);
}

.settings-section {
  padding: 10px 12px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}

.section-title {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.4);
  font-weight: 500;
  letter-spacing: 0.5px;
  margin-bottom: 6px;
}

.section-subtitle {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.4);
  font-weight: 500;
  letter-spacing: 0.5px;
  margin-top: 10px;
  margin-bottom: 6px;
}

.mode-toggle {
  display: flex;
  align-items: center;
  border: 1px solid color-mix(in srgb, var(--focus-color) 82%, transparent);
  border-radius: 999px;
  overflow: hidden;
  width: fit-content;
  max-width: 100%;
}

.mode-btn {
  appearance: none;
  border: none;
  background: transparent;
  color: rgba(255, 255, 255, 0.85);
  font-size: 12px;
  font-weight: 600;
  line-height: 1;
  height: 28px;
  padding: 0 14px;
  cursor: pointer;
  transition: background 0.22s ease, color 0.22s ease;
  white-space: nowrap;
}

.mode-btn + .mode-btn {
  border-left: 1px solid color-mix(in srgb, var(--focus-color) 58%, transparent);
}

.mode-btn:hover {
  background: color-mix(in srgb, var(--focus-color) 15%, transparent);
}

.mode-btn.active {
  background: var(--focus-color);
  color: #fff;
}

/* 更新 */
.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.setting-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.update-toggle-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.update-copy {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.update-label {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.78);
}

.update-hint {
  font-size: 10px;
  line-height: 1.4;
  color: rgba(255, 255, 255, 0.36);
}

.switch {
  position: relative;
  display: inline-flex;
  align-items: center;
  width: 42px;
  height: 24px;
  flex-shrink: 0;
  cursor: pointer;
}

.switch input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}

.switch-track {
  position: relative;
  width: 42px;
  height: 24px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.12);
  border: 1px solid rgba(255, 255, 255, 0.14);
  transition: background 0.2s ease, border-color 0.2s ease;
}

.switch-track::after {
  content: '';
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.82);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.35);
  transition: transform 0.2s ease, background 0.2s ease;
}

.switch input:checked + .switch-track {
  background: color-mix(in srgb, var(--focus-color) 72%, rgba(255, 255, 255, 0.12));
  border-color: color-mix(in srgb, var(--focus-color) 88%, transparent);
}

.switch input:checked + .switch-track::after {
  transform: translateX(18px);
  background: #fff;
}

.switch input:focus-visible + .switch-track {
  outline: 2px solid color-mix(in srgb, var(--focus-color) 80%, white);
  outline-offset: 2px;
}

.mirror-sources {
  margin-top: 8px;
}

.mirror-sources-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.source-probe-btn {
  border: none;
  background: transparent;
  color: color-mix(in srgb, var(--focus-color) 70%, #fff);
  font-size: 11px;
  font-family: inherit;
  padding: 2px 5px;
  border-radius: 4px;
  cursor: pointer;
  white-space: nowrap;
}

.source-probe-btn:hover {
  background: rgba(255, 255, 255, 0.06);
}

.source-probe-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.source-status {
  display: block;
  margin-top: 4px;
  font-size: 10px;
  color: rgba(255, 255, 255, 0.42);
}

.row-spaced {
  margin-top: 8px;
}

.setting-option-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 8px;
}

.mirror-empty {
  display: block;
  margin-top: 4px;
  font-size: 10px;
  color: rgba(255, 255, 255, 0.42);
}

.mirror-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 6px;
}

.mirror-chip {
  appearance: none;
  max-width: 170px;
  padding: 3px 9px;
  border-radius: 999px;
  border: 1px solid rgba(255, 255, 255, 0.10);
  background: rgba(255, 255, 255, 0.04);
  color: rgba(255, 255, 255, 0.62);
  font-size: 11px;
  font-family: inherit;
  line-height: 1.4;
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  transition: all 0.15s ease;
}

.mirror-chip:hover {
  background: rgba(255, 255, 255, 0.08);
  color: #fff;
}

.mirror-chip.muted {
  border-style: dashed;
  color: rgba(255, 255, 255, 0.35);
  text-decoration: line-through;
}

.mirror-chip.muted:hover {
  color: rgba(255, 255, 255, 0.7);
}

.mirror-chip.selected {
  border-color: color-mix(in srgb, var(--focus-color) 55%, transparent);
  background: color-mix(in srgb, var(--focus-color) 12%, transparent);
  color: color-mix(in srgb, var(--focus-color) 85%, #fff);
}

.mirror-chip.selected:hover {
  background: color-mix(in srgb, var(--focus-color) 18%, transparent);
  color: color-mix(in srgb, var(--focus-color) 95%, #fff);
}

.collapsible-row {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 0;
  border: none;
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
  font-family: inherit;
}

.collapsible-row:hover .update-label {
  color: #fff;
}

.collapsible-chevron {
  width: 14px;
  height: 14px;
  margin-right: 3px;
  color: rgba(255, 255, 255, 0.35);
  flex-shrink: 0;
  transition: transform 0.2s ease;
}

.collapsible-chevron.open {
  transform: rotate(180deg);
}

.email-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 8px;
}

.email-input {
  width: 100%;
  box-sizing: border-box;
  padding: 7px 10px;
  border-radius: 7px;
  border: 1px solid rgba(255, 255, 255, 0.10);
  background: rgba(255, 255, 255, 0.04);
  color: #fff;
  font-size: 12px;
  font-family: inherit;
  outline: none;
  transition: border-color 0.15s ease, background 0.15s ease;
}

.email-input::placeholder {
  color: rgba(255, 255, 255, 0.35);
}

.email-input:focus {
  border-color: color-mix(in srgb, var(--focus-color) 50%, transparent);
  background: rgba(255, 255, 255, 0.06);
}

.email-input.email-select {
  appearance: none;
  -webkit-appearance: none;
  cursor: pointer;
  padding-right: 30px;
  background-color: rgba(255, 255, 255, 0.06);
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 10px center;
  background-size: 12px;
}

.email-input.email-select:focus {
  border-color: color-mix(in srgb, var(--focus-color) 50%, transparent);
  background-color: rgba(255, 255, 255, 0.08);
}

.email-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 4px;
}

.email-status {
  margin-top: 2px;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.5);
  line-height: 1.4;
}

.email-status.error {
  color: #f87171;
}

.email-status.success {
  color: #4ade80;
}

.email-hint {
  margin-top: 4px;
  padding-top: 8px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
  font-size: 10px;
  color: rgba(255, 255, 255, 0.35);
  line-height: 1.5;
}

.update-action-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 8px;
  min-width: 0;
}

.update-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-height: 28px;
  padding: 0 10px;
  border-radius: 6px;
  border: 1px solid rgba(255, 255, 255, 0.13);
  background: rgba(255, 255, 255, 0.06);
  color: rgba(255, 255, 255, 0.82);
  font-size: 12px;
  font-weight: 600;
  font-family: inherit;
  cursor: pointer;
  transition: background 0.2s ease, border-color 0.2s ease, color 0.2s ease;
}

.update-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  border-color: rgba(255, 255, 255, 0.24);
  color: #fff;
}

.update-btn.primary {
  background: var(--focus-color);
  border-color: var(--focus-color);
  color: #fff;
}

.update-btn.danger {
  background: rgba(248, 113, 113, 0.06);
  border-color: rgba(248, 113, 113, 0.22);
  color: rgba(248, 113, 113, 0.9);
}

.update-btn.danger:hover:not(:disabled) {
  background: rgba(248, 113, 113, 0.14);
  border-color: rgba(248, 113, 113, 0.4);
  color: #fca5a5;
}

.update-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.update-status {
  margin-top: 4px;
  font-size: 10px;
  line-height: 1.4;
  color: rgba(255, 255, 255, 0.42);
  min-width: 0;
  white-space: pre-wrap;
  word-break: break-word;
}

.update-status.success {
  color: #4ade80;
}

.update-status.error {
  color: #f87171;
}

.update-recovery {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 7px;
  font-size: 10px;
  line-height: 1.4;
  color: rgba(255, 255, 255, 0.48);
}

.update-link-btn {
  appearance: none;
  border: none;
  background: transparent;
  color: var(--focus-color);
  font: inherit;
  font-weight: 700;
  padding: 2px 0;
  cursor: pointer;
}

.update-link-btn:hover {
  color: color-mix(in srgb, var(--focus-color) 72%, white);
}

.mini-spinner {
  width: 11px;
  height: 11px;
  border: 2px solid rgba(255, 255, 255, 0.2);
  border-top-color: currentColor;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* 外观 - 滑块行 */
.slider-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 6px;
}

.slider-row:last-child {
  margin-bottom: 0;
}

.slider-label {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.7);
  width: 40px;
  flex-shrink: 0;
}

.opacity-slider {
  flex: 1;
  appearance: none;
  height: 3px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.12);
  outline: none;
}

.opacity-slider::-webkit-slider-thumb {
  appearance: none;
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: #fff;
  cursor: pointer;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
}

.opacity-value {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.7);
  min-width: 32px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

/* 番茄钟 */
.duration-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.duration-cell {
  display: flex;
  align-items: center;
  gap: 8px;
}

.duration-label {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.7);
  flex-shrink: 0;
}

.stepper {
  display: flex;
  align-items: center;
  gap: 0;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 6px;
  overflow: hidden;
}

.stepper-btn {
  background: none;
  border: none;
  color: rgba(255, 255, 255, 0.7);
  font-size: 14px;
  width: 24px;
  height: 24px;
  cursor: pointer;
  transition: background 0.2s, color 0.2s;
  display: flex;
  align-items: center;
  justify-content: center;
}

.stepper-btn:hover {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}

.stepper-value {
  font-size: 13px;
  font-weight: 600;
  color: #fff;
  min-width: 26px;
  text-align: center;
  font-variant-numeric: tabular-nums;
}

.duration-unit {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.4);
}

/* 快捷操作 */
.shortcut-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 4px;
}

.shortcut-label {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.7);
}

.shortcut-recorder {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 120px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.06);
  color: rgba(255, 255, 255, 0.85);
  font-size: 12px;
  font-weight: 600;
  font-family: inherit;
  cursor: pointer;
  transition: all 0.2s;
}

.shortcut-recorder:hover {
  background: rgba(255, 255, 255, 0.10);
  border-color: rgba(255, 255, 255, 0.25);
}

.shortcut-recorder.recording {
  border-color: var(--focus-color);
  background: color-mix(in srgb, var(--focus-color) 15%, transparent);
  box-shadow: 0 0 0 1px var(--focus-color);
  animation: recorder-pulse 1.2s ease-in-out infinite;
}

@keyframes recorder-pulse {
  0%, 100% { box-shadow: 0 0 0 1px var(--focus-color); }
  50% { box-shadow: 0 0 0 2px var(--focus-color), 0 0 8px color-mix(in srgb, var(--focus-color) 40%, transparent); }
}

.recording-hint {
  color: var(--focus-color);
  font-weight: 500;
}

.shortcut-hint {
  font-size: 10px;
  color: rgba(255, 255, 255, 0.35);
  white-space: nowrap;
}

/* 主题 */
.theme-grid {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 6px;
}

.theme-card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 5px;
  padding: 8px 4px;
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: rgba(255, 255, 255, 0.03);
  cursor: pointer;
  transition: all 0.2s;
  text-align: center;
}

.theme-card:hover {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.12);
}

.theme-card.active {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.25);
}

.theme-colors {
  display: flex;
  gap: 3px;
}

.color-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
}

.theme-name {
  font-size: 10px;
  color: rgba(255, 255, 255, 0.75);
  line-height: 1.2;
  white-space: nowrap;
}

.theme-check {
  position: absolute;
  top: 2px;
  right: 4px;
  font-size: 10px;
  color: var(--focus-color);
  font-weight: 700;
}

.settings-page::-webkit-scrollbar { width: 4px; }
.settings-page::-webkit-scrollbar-track { background: transparent; }
.settings-page::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.12); border-radius: 2px; }

/* 毛玻璃模糊效果 */
.settings-page.blurred {
  filter: blur(8px);
  opacity: 0.7;
  transition: filter 0.2s ease-out, opacity 0.2s ease-out;
}
.motto-editor {
  width: 100%;
  box-sizing: border-box;
  resize: vertical;
  min-height: 76px;
  border: 1px solid rgba(255, 255, 255, 0.10);
  background: rgba(255, 255, 255, 0.05);
  border-radius: 8px;
  color: inherit;
  font: inherit;
  font-size: 11px;
  line-height: 1.6;
  padding: 8px 9px;
  outline: none;
}

.motto-editor:focus {
  border-color: rgba(255, 255, 255, 0.25);
}

.motto-hint {
  font-size: 10px;
  opacity: 0.55;
  margin-top: 4px;
}
</style>
