<script setup lang="ts">
import { computed, ref, watch, onBeforeUnmount, onDeactivated } from 'vue'
import { useDebounceFn, useWindowSize } from '@vueuse/core'
import { useTasks, type Task, isTaskDoneOnDate, todayString } from '../composables/useTasks'
import { useTimerBridge } from '../composables/useTimerBridge'
import { useTaskGroups } from '../composables/useTaskGroups'
import { useSettings } from '../composables/useSettings'
import { getCategoryColor } from '../composables/useMemos'
import { PIXEL_5X7, layoutPixelText } from '../utils/pixelFont'

const emit = defineEmits<{ close: [] }>()

const { groups } = useTaskGroups()

const {
  tasks,
  recentFocusTasks: recentTasks,
  addTask,
  setTaskPriority,
  incrementPomodoro,
  toggleComplete,
} = useTasks()
const {
  start,
  pause,
  resume,
  running,
  activeTaskId,
  activeTaskTitle,
  displayTime,
  progress,
  totalDuration,
  phase,
  abandon,
} = useTimerBridge()
const { settings } = useSettings()

// 快速专注：不依赖 TODO，直接用虚拟任务启动一个番茄
const QUICK_TASK_ID = 'quick-focus'
const QUICK_TASK_TITLE = '快速专注'
const quickActive = computed(() => activeTaskId.value === QUICK_TASK_ID)

function startQuick() {
  if (activeTaskId.value) return
  start(QUICK_TASK_ID, QUICK_TASK_TITLE)
}

// ---- 首页计时英雄区 ----
// 面板打开动画会短暂改变窗口宽度；等宽度稳定后再切换像素档位，避免数字在打开瞬间跳动。
const { width: rawViewportWidth } = useWindowSize()
const viewportWidth = ref(420)
const syncViewportWidth = useDebounceFn((width: number) => {
  viewportWidth.value = width
}, 500)
watch(rawViewportWidth, (width) => syncViewportWidth(width), { immediate: true })
const pixelUnit = computed(() => (viewportWidth.value < 350 ? 6 : viewportWidth.value < 400 ? 7 : 8))
const pixelTime = computed(() => layoutPixelText(displayTime.value, PIXEL_5X7, pixelUnit.value, 1))
const progressBars = computed(() =>
  Array.from({ length: 40 }, (_, index) => index < Math.round(progress.value * 40)),
)
const heroTitle = computed(() => {
  if (activeTaskId.value) return activeTaskTitle.value ?? '专注中'
  return '准备开始一段专注'
})
const heroStatus = computed(() => {
  if (activeTaskId.value) return running.value ? '进行中' : '已暂停'
  return '待开始'
})
const heroMinutes = computed(() => Math.round(totalDuration.value / 60))

function handleHeroAction() {
  if (activeTaskId.value) {
    if (running.value) pause()
    else resume()
    return
  }
  startQuick()
}

// --- Quick-add state ---
const newTitle = ref('')
const quickAddRef = ref<HTMLInputElement | null>(null)

// --- Title scroll animation ---
type ScrollPhase = 'start' | 'scroll' | 'end'
interface ScrollState {
  taskId: string
  container: HTMLElement
  text: HTMLElement
  frameId: number | null
  offset: number
  waitFrames: number
  phase: ScrollPhase
}

const SCROLL_SPEED = 0.5
const START_WAIT_FRAMES = 30
const END_WAIT_FRAMES = 60
let activeScroll: ScrollState | null = null

function startScroll(taskId: string, eventTarget: EventTarget | null) {
  const containerEl = eventTarget as HTMLElement | null
  if (!containerEl) return
  const textEl = containerEl.querySelector('.task-title-text') as HTMLElement | null
  if (!textEl) return
  const overflow = textEl.scrollWidth - containerEl.clientWidth
  if (overflow <= 0) return
  stopScroll()
  textEl.style.transform = 'translateX(0)'
  activeScroll = { taskId, container: containerEl, text: textEl, frameId: null, offset: 0, waitFrames: START_WAIT_FRAMES, phase: 'start' }
  scheduleNextScrollFrame()
}

function scheduleNextScrollFrame() {
  if (!activeScroll) return
  activeScroll.frameId = requestAnimationFrame(runScrollFrame)
}

function runScrollFrame() {
  const state = activeScroll
  if (!state) return
  if (!state.container.isConnected || !state.text.isConnected) { stopScroll(state.taskId); return }
  const overflow = state.text.scrollWidth - state.container.clientWidth
  if (overflow <= 0) { stopScroll(state.taskId); return }
  if (state.phase !== 'scroll') {
    state.waitFrames -= 1
    if (state.waitFrames <= 0) {
      if (state.phase === 'end') { state.offset = 0; state.text.style.transform = 'translateX(0)' }
      state.phase = 'scroll'
    }
    scheduleNextScrollFrame(); return
  }
  state.offset += SCROLL_SPEED
  if (state.offset >= overflow) {
    state.offset = overflow
    state.text.style.transform = `translateX(-${overflow}px)`
    state.phase = 'end'
    state.waitFrames = END_WAIT_FRAMES
  } else {
    state.text.style.transform = `translateX(-${state.offset}px)`
  }
  scheduleNextScrollFrame()
}

function stopScroll(taskId?: string) {
  if (!activeScroll) return
  if (taskId && activeScroll.taskId !== taskId) return
  if (activeScroll.frameId !== null) cancelAnimationFrame(activeScroll.frameId)
  activeScroll.text.style.transform = 'translateX(0)'
  activeScroll = null
}

onDeactivated(() => { stopScroll() })
onBeforeUnmount(() => { stopScroll() })

// --- Quick-add ---
async function handleInput(event: KeyboardEvent) {
  if (event.key === 'Enter') {
    event.preventDefault()
    const t = newTitle.value.trim()
    if (!t) return
    const task = addTask(t)
    newTitle.value = ''
    if (event.shiftKey) {
      setTaskPriority(task.id, 1)
      start(task.id, task.title)
      emit('close')
    }
  }
}

// --- Timer Controls ---
function handleStartTask(task: Task) {
  if (activeTaskId.value === task.id) {
    if (running.value) pause()
    else resume()
  } else {
    start(task.id, task.title)
  }
}

function isDoneToday(task: Task): boolean {
  return isTaskDoneOnDate(task, todayString.value)
}

function handleDoneTask(taskId: string) {
  const task = tasks.value.find(item => item.id === taskId)
  const doneToday = task ? isDoneToday(task) : false
  if (activeTaskId.value === taskId) {
    // 已完成的任务再次点击是撤销完成，不再重复累计番茄。
    if (!doneToday) incrementPomodoro(taskId)
    abandon()
  }
  toggleComplete(taskId)
}
</script>

<template>
  <div class="home-page">
    <!-- Timer hero -->
    <section class="focus-hero" tabindex="-1" aria-label="专注计时">
      <div class="hero-eyebrow">
        <span class="pixel-status" aria-hidden="true"></span>
        <span>{{ phase === 'break' ? '休息' : '专注' }}</span>
      </div>
      <h1>{{ heroTitle }}</h1>
      <svg
        class="pixel-time"
        :width="pixelTime.width"
        :height="pixelTime.height"
        :viewBox="`0 0 ${pixelTime.width} ${pixelTime.height}`"
        shape-rendering="crispEdges"
        :aria-label="`剩余 ${displayTime}`"
      >
        <rect
          v-for="cell in pixelTime.cells"
          :key="`${cell.x}-${cell.y}`"
          :x="cell.x"
          :y="cell.y"
          :width="pixelUnit"
          :height="pixelUnit"
        />
      </svg>
      <div class="pixel-progress" aria-hidden="true">
        <i v-for="(filled, index) in progressBars" :key="index" :class="{ filled }" />
      </div>
      <div class="hero-meta">
        <span>{{ heroMinutes }} 分钟</span>
        <span>{{ heroStatus }}</span>
      </div>
      <div class="hero-actions">
        <button class="primary-action" @click="handleHeroAction">
          {{ activeTaskId ? (running ? '暂停' : '继续') : '开始专注' }}
        </button>
        <button v-if="activeTaskId" class="quiet-action" @click="abandon()">停止</button>
      </div>
    </section>

    <!-- Zone 2: Focus Queue (Recent 3) -->
    <div class="focus-zone">
      <div class="zone-header">核心专注区</div>

      <div class="focus-container">
        <!-- 快速专注运行态：与任务同一块核心专注区 -->
        <div
          v-if="quickActive"
          class="focus-task-item is-running"
          :style="{ '--active-color': phase === 'break' ? 'var(--break-color)' : 'var(--focus-color)' }"
        >
          <div class="running-card">
            <div class="running-header">
              <span class="running-dot">●</span>
              <span class="running-title">{{ phase === 'break' ? '休息中' : '快速专注' }}</span>
              <span class="task-timer">{{ displayTime }}</span>
            </div>
            <div class="running-actions">
              <button class="run-btn pause-btn" @click="running ? pause() : resume()">
                {{ running ? '❚❚ 暂停' : '▶ 继续' }}
              </button>
              <button class="run-btn abandon-btn" @click="abandon()">■ 停止</button>
            </div>
          </div>
        </div>

        <div v-if="recentTasks.length === 0 && !quickActive" class="focus-empty">
          <span class="focus-empty-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
          </span>
          <span class="focus-empty-text">还没有任务，点右上角 ▶ 直接开始</span>
        </div>

        <div
          v-for="task in recentTasks"
          :key="task.id"
          class="focus-task-item"
          :class="{ 'is-running': activeTaskId === task.id }"
          :style="activeTaskId === task.id ? { '--active-color': phase === 'break' ? 'var(--break-color)' : 'var(--focus-color)' } : undefined"
        >
        <!-- 运行态 -->
        <template v-if="activeTaskId === task.id">
          <template v-if="running">
            <div class="running-card">
              <div class="running-header">
                <span class="running-dot">●</span>
                <span class="task-title running-title" @mouseenter="startScroll(task.id, $event.currentTarget)" @mouseleave="stopScroll(task.id)">
                  <span class="task-title-text">{{ task.title }}</span>
                </span>
                <span v-if="task.groupId" class="task-group-badge">
                  <span class="group-dot-sm" :style="{ backgroundColor: getCategoryColor(groups.find(g => g.id === task.groupId)?.color ?? 'yellow').icon }"></span>
                  {{ groups.find(g => g.id === task.groupId)?.name ?? '' }}
                </span>
                <span class="task-timer">{{ displayTime }}</span>
              </div>
              <div class="running-actions">
                <button class="run-btn pause-btn" @click="pause()" title="暂停">❚❚ 暂停</button>
                <button class="run-btn done-btn" @click="handleDoneTask(task.id)" title="完成">✓ 完成</button>
                <button class="run-btn abandon-btn" @click="abandon()" title="停止">■ 停止</button>
              </div>
            </div>
          </template>
          <!-- 暂停态 -->
          <template v-else>
            <div class="running-card paused">
              <div class="running-header">
                <span class="running-dot paused-dot">●</span>
                <span class="task-title running-title" @mouseenter="startScroll(task.id, $event.currentTarget)" @mouseleave="stopScroll(task.id)">
                  <span class="task-title-text">{{ task.title }}</span>
                </span>
                <span v-if="task.groupId" class="task-group-badge">
                  <span class="group-dot-sm" :style="{ backgroundColor: getCategoryColor(groups.find(g => g.id === task.groupId)?.color ?? 'yellow').icon }"></span>
                  {{ groups.find(g => g.id === task.groupId)?.name ?? '' }}
                </span>
                <span class="task-timer">{{ displayTime }}</span>
              </div>
              <div class="running-actions">
                <button class="run-btn pause-btn" @click="resume()" title="继续">▶ 继续</button>
                <button class="run-btn done-btn" @click="handleDoneTask(task.id)" title="完成">✓ 完成</button>
                <button class="run-btn abandon-btn" @click="abandon()" title="停止">■ 停止</button>
              </div>
            </div>
          </template>
        </template>

        <!-- 正常态（非活跃任务） -->
        <template v-else>
          <div class="task-row">
            <button
              class="check-circle"
              @click="handleDoneTask(task.id)"
              title="完成"
              aria-label="完成"
            ></button>
            <span class="task-title" @mouseenter="startScroll(task.id, $event.currentTarget)" @mouseleave="stopScroll(task.id)">
              <span class="task-title-text">{{ task.title }}</span>
            </span>
            <span v-if="task.groupId" class="task-group-badge">
              <span class="group-dot-sm" :style="{ backgroundColor: getCategoryColor(groups.find(g => g.id === task.groupId)?.color ?? 'yellow').icon }"></span>
              {{ groups.find(g => g.id === task.groupId)?.name ?? '' }}
            </span>
            <span class="pomo-count" v-if="task.pomodoroCount > 0">● {{ task.pomodoroCount }}</span>
            <button class="start-btn" @click="handleStartTask(task)" title="开始">▶</button>
          </div>
        </template>
      </div>
      </div>
    </div>

    <!-- Zone 3: Quick-add -->
    <div class="quick-add-zone">
      <div class="quick-add-row">
        <div class="quick-add-search">
          <svg class="search-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="11" cy="11" r="7"></circle>
            <line x1="16.5" y1="16.5" x2="21" y2="21"></line>
          </svg>
          <input
            ref="quickAddRef"
            v-model="newTitle"
            class="quick-add-field-input"
            placeholder="添加任务 · Enter 保存"
            title="Enter 保存，Shift+Enter 直接开始"
            @keydown="handleInput"
          />
        </div>
        <button
          class="quick-add-btn"
          :disabled="!!activeTaskId"
          :title="`无需任务，直接开始 ${settings.focusDuration} 分钟专注`"
          @click="startQuick"
        >
          ▶ {{ settings.focusDuration }}′
        </button>
      </div>
    </div>

  </div>
</template>

<style scoped>
@reference "../styles.css";

/* ===== V2 计时英雄区 ===== */
.focus-hero {
  position: relative;
  flex-shrink: 0;
  padding: 16px;
  border-radius: 18px;
  background:
    radial-gradient(ellipse at 80% 15%, var(--v2-glow), transparent 66%),
    var(--v2-surface);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.28),
    0 8px 24px var(--v2-shadow);
  color: var(--v2-ink);
}

.hero-eyebrow {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--v2-secondary);
  font-size: 12px;
}

.pixel-status {
  width: 8px;
  height: 8px;
  background: var(--v2-accent);
  clip-path: polygon(
    25% 0,
    75% 0,
    75% 25%,
    100% 25%,
    100% 75%,
    75% 75%,
    75% 100%,
    25% 100%,
    25% 75%,
    0 75%,
    0 25%,
    25% 25%
  );
}

.focus-hero h1 {
  margin: 12px 0 16px;
  color: var(--v2-ink);
  font-size: 20px;
  font-weight: 600;
  line-height: 1.35;
  letter-spacing: -0.5px;
  overflow-wrap: anywhere;
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
}

.pixel-time {
  display: block;
  margin: 0 auto 16px;
  image-rendering: pixelated;
  color: var(--v2-accent);
}

.pixel-time rect {
  fill: var(--v2-accent);
}

.pixel-progress {
  display: grid;
  grid-template-columns: repeat(40, 4px);
  justify-content: center;
  gap: 4px;
  height: 4px;
  margin: 0 0 12px;
}

.pixel-progress i {
  width: 4px;
  height: 4px;
  background: var(--v2-line);
}

.pixel-progress i.filled {
  background: var(--v2-accent);
}

.hero-meta {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 12px;
  color: var(--v2-secondary);
  font-size: 12px;
}

.hero-actions {
  display: flex;
  gap: 8px;
}

.hero-actions button {
  min-height: 36px;
  padding: 8px 12px;
  border: 0;
  border-radius: 9px;
  background: var(--ui-well);
  color: var(--v2-secondary);
  font-size: 12px;
  font-family: inherit;
  cursor: pointer;
}

.hero-actions .primary-action {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 14px;
  background: var(--v2-accent);
  color: var(--v2-on-accent);
  font-weight: 600;
  /* 像素质感的硬边投影：不模糊、不发光 */
  box-shadow: 3px 3px 0 color-mix(in srgb, var(--v2-accent) 22%, transparent);
}

.hero-actions .primary-action:hover {
  filter: brightness(1.04);
}

.hero-actions .quiet-action:hover {
  background: var(--ui-hover);
  color: var(--v2-ink);
}

@media (max-width: 399px) {
  .pixel-progress {
    gap: 3px;
  }
}

@media (max-width: 349px) {
  .focus-hero {
    padding: 16px 12px;
  }

  .pixel-progress {
    gap: 2px;
  }

  .hero-meta {
    flex-wrap: wrap;
  }
}

.home-page {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  height: 100%;
  overflow-y: auto;
  background: transparent;
  color: var(--v2-ink);
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
}

.home-page::-webkit-scrollbar {
  width: 4px;
}
.home-page::-webkit-scrollbar-track {
  background: transparent;
}
.home-page::-webkit-scrollbar-thumb {
  background: var(--ui-hover);
  border-radius: 2px;
}

/* ===== Quick-add Zone ===== */
.quick-add-zone {
  flex-shrink: 0;
}

/* 添加任务：沿用搜索框的圆角/内距，输入区和按钮各占自己的位置，长文本不会互相压 */
.quick-add-search {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  height: 38px;
  padding: 0 12px;
  background: var(--ui-well);
  border: 1px solid var(--v2-line);
  border-radius: 9px;
  transition: border-color 0.2s, box-shadow 0.2s, background 0.2s;
}

.quick-add-search:focus-within {
  border-color: color-mix(in srgb, var(--v2-accent) 55%, transparent);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--v2-accent) 12%, transparent);
  background: color-mix(in srgb, var(--v2-accent) 7%, var(--ui-well));
}

.quick-add-search .search-icon {
  flex-shrink: 0;
  color: var(--v2-muted);
}

.quick-add-field-input {
  flex: 1 1 0;
  min-width: 0;
  height: 100%;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--v2-ink);
  font-size: 13px;
  font-family: inherit;
}

.quick-add-field-input::placeholder {
  color: var(--v2-muted);
}

.quick-add-input {
  width: 100%;
  background: var(--ui-well);
  border: 1px solid var(--v2-line);;
  border-radius: 8px;
  padding: 10px 14px;
  color: var(--v2-ink);
  font-size: 13px;
  outline: none;
  transition: all 0.2s;
  box-sizing: border-box;
  font-family: inherit;
}

.quick-add-input:focus {
  border-color: color-mix(in srgb, var(--focus-color) 60%, transparent);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--focus-color) 20%, transparent);
}

.quick-add-input::placeholder {
  color: var(--v2-muted);
}

/* ===== Focus Zone ===== */
/* ===== Quick Focus Zone ===== */
.quick-focus-zone {
  display: none;
}

.quick-focus-btn {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 9px 12px;
  border-radius: 8px;
  border: 1px solid color-mix(in srgb, var(--focus-color) 30%, transparent);
  background: color-mix(in srgb, var(--focus-color) 10%, rgba(255, 255, 255, 0.03));
  color: var(--v2-ink);
  cursor: pointer;
  font-family: inherit;
  transition: background 0.2s, border-color 0.2s;
}

.quick-focus-btn:hover {
  background: color-mix(in srgb, var(--focus-color) 18%, rgba(255, 255, 255, 0.04));
  border-color: color-mix(in srgb, var(--focus-color) 45%, transparent);
}

.quick-focus-title {
  font-size: 12px;
  font-weight: 600;
}

.quick-focus-meta {
  font-size: 10px;
  color: var(--v2-muted);
}

.quick-running {
  padding: 6px 10px;
  border-radius: 8px;
  background: var(--ui-well);
  border: 1px solid var(--v2-line);;
}

.focus-zone {
  flex-shrink: 0;
}

.zone-header {
  font-size: 11px;
  color: var(--v2-muted);
  margin-bottom: 6px;
  font-weight: 500;
  letter-spacing: 0.5px;
}

.focus-container {
  background: var(--ui-well);
  border: 1px dashed var(--v2-line);;
  border-radius: 10px;
  padding: 8px;
}

.focus-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 16px;
}

.focus-empty-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border-radius: 50%;
  background: var(--ui-well);
  color: var(--v2-muted);
}

.focus-empty-text {
  font-size: 12px;
  color: var(--v2-muted);
  text-align: center;
}

.focus-task-item {
  background: var(--ui-well);
  border-radius: 8px;
  margin-bottom: 4px;
  padding: 5px 10px;
  transition: all 0.2s;
  border: 1px solid var(--v2-line);;
}

.focus-task-item:last-child {
  margin-bottom: 0;
}

.focus-task-item:hover {
  background: var(--ui-well);
  border-color: var(--v2-muted);
}

/* Running task card */
.focus-task-item.is-running {
  background: color-mix(in srgb, var(--active-color) 10%, transparent);
  border-color: color-mix(in srgb, var(--active-color) 30%, transparent);
  border-left: 3px solid var(--active-color);
  padding: 8px 10px;
}

.running-card {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.running-header {
  display: flex;
  align-items: center;
  gap: 8px;
}

.running-dot {
  color: var(--active-color);
  font-size: 10px;
  flex-shrink: 0;
}

.paused-dot {
  animation: blink 1.2s ease-in-out infinite;
}

@keyframes blink {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}

.running-title {
  color: var(--v2-ink);
  font-weight: 500;
}

.running-title .task-title-text {
  color: var(--v2-ink);
  font-weight: 500;
}

.running-actions {
  display: flex;
  gap: 6px;
  position: relative;
  z-index: 2;
  pointer-events: auto;
}

.run-btn {
  height: 28px;
  padding: 0 10px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  border: 1px solid var(--v2-line);;
  transition: all 0.2s;
  font-family: inherit;
}

.pause-btn {
  flex: 1;
  background: var(--ui-hover);
  color: var(--v2-ink);
}

.pause-btn:hover {
  background: var(--ui-hover);
  color: var(--v2-ink);
}

.done-btn {
  background: color-mix(in srgb, var(--break-color) 20%, transparent);
  color: var(--break-color);
  border-color: color-mix(in srgb, var(--break-color) 30%, transparent);
}

.done-btn:hover {
  background: color-mix(in srgb, var(--break-color) 35%, transparent);
}

.abandon-btn {
  background: transparent;
  color: var(--v2-muted);
  border-color: var(--v2-muted);
}

.abandon-btn:hover {
  color: #f87171;
  border-color: rgba(248, 113, 113, 0.3);
  background: rgba(248, 113, 113, 0.1);
}

/* Normal task row */
.task-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 32px;
}

.rank-badge {
  width: 16px;
  height: 16px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  line-height: 1;
  flex-shrink: 0;
  color: var(--focus-color);
  background: color-mix(in srgb, var(--focus-color) 15%, transparent);
  border-radius: 50%;
}

.check-circle {
  position: relative;
  display: grid;
  place-items: center;
  width: 16px;
  height: 16px;
  border-radius: 999px;
  border: 2px solid var(--v2-line);;
  background: transparent;
  cursor: pointer;
  padding: 0;
  flex-shrink: 0;
  transition: border-color 0.2s, background 0.2s;
}

/* 视觉尺寸保持紧凑，但把可点击范围扩到约 28px */
.check-circle::before {
  content: '';
  position: absolute;
  inset: -6px;
  border-radius: 50%;
}

.check-circle:hover {
  border-color: var(--break-color);
  background: color-mix(in srgb, var(--break-color) 20%, transparent);
}

.task-title {
  flex: 1;
  font-size: 13px;
  color: var(--v2-ink);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  position: relative;
  min-width: 0;
  cursor: default;
}

.task-title-text {
  display: inline-block;
  will-change: transform;
}

.pomo-count {
  font-size: 11px;
  color: var(--focus-color);
  opacity: 0.7;
}

.task-timer {
  font-size: 13px;
  font-weight: 700;
  color: var(--active-color);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}

.task-actions {
  display: flex;
  gap: 4px;
  opacity: 0;
  transition: opacity 0.15s;
}

.focus-task-item:hover .task-actions {
  opacity: 1;
}

/* Unified action button */
.act-btn {
  min-width: 20px;
  height: 20px;
  padding: 1px 4px;
  border-radius: 5px;
  font-size: 10px;
  font-weight: 600;
  background: var(--ui-well);
  border: 1px solid var(--v2-line);;
  color: var(--v2-ink);
  cursor: pointer;
  text-align: center;
  font-family: inherit;
  transition: background 0.2s, color 0.2s, border-color 0.2s;
}

.act-btn:hover {
  background: var(--ui-hover);
  border-color: var(--v2-muted);
  color: var(--v2-ink);
}

.start-btn {
  min-width: 20px;
  height: 20px;
  padding: 1px 4px;
  border-radius: 5px;
  font-size: 10px;
  font-weight: 600;
  background: color-mix(in srgb, var(--focus-color) 15%, transparent);
  border: 1px solid color-mix(in srgb, var(--focus-color) 30%, transparent);
  color: var(--focus-color);
  cursor: pointer;
  text-align: center;
  flex-shrink: 0;
  font-family: inherit;
  transition: background 0.2s, color 0.2s, border-color 0.2s;
}

.start-btn:hover {
  background: color-mix(in srgb, var(--focus-color) 25%, transparent);
  border-color: color-mix(in srgb, var(--focus-color) 50%, transparent);
}

.act-btn.delete {
  color: var(--v2-muted);
}

.act-btn.delete:hover {
  color: #f87171;
  border-color: rgba(248, 113, 113, 0.3);
  background: rgba(248, 113, 113, 0.1);
}

.task-time {
  font-size: 10px;
  color: var(--v2-muted);
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}

/* ===== Radio Zone ===== */
.radio-zone {
  flex-shrink: 0;
}

.radio-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border-radius: 10px;
  background: linear-gradient(135deg, rgba(255, 255, 255, 0.04) 0%, rgba(255, 255, 255, 0.02) 100%);
  border: 1px solid var(--v2-line);;
  transition: all 0.2s;
}

.radio-play-btn {
  width: 40px;
  height: 40px;
  border-radius: 50%;
  background: var(--ui-well);
  border: 1px solid var(--v2-line);;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--v2-secondary);
  flex-shrink: 0;
  cursor: pointer;
  transition: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);
  padding: 0;
}

.radio-play-btn:hover {
  background: var(--ui-hover);
  border-color: var(--v2-muted);
  color: var(--v2-ink);
  transform: scale(1.05);
}

.radio-play-btn:active {
  transform: scale(0.95);
}

.radio-play-btn.playing {
  background: linear-gradient(135deg, color-mix(in srgb, var(--focus-color) 25%, transparent), color-mix(in srgb, var(--focus-color) 15%, transparent));
  border-color: color-mix(in srgb, var(--focus-color) 40%, transparent);
  color: var(--focus-color);
  box-shadow: 0 0 12px color-mix(in srgb, var(--focus-color) 20%, transparent);
}

.radio-play-btn.playing:hover {
  background: linear-gradient(135deg, color-mix(in srgb, var(--focus-color) 35%, transparent), color-mix(in srgb, var(--focus-color) 25%, transparent));
  border-color: color-mix(in srgb, var(--focus-color) 50%, transparent);
  box-shadow: 0 0 16px color-mix(in srgb, var(--focus-color) 30%, transparent);
}

.loading-spinner {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.radio-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.radio-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--v2-ink);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.radio-status {
  font-size: 11px;
  color: var(--v2-muted);
  font-weight: 500;
}

.radio-wave {
  display: flex;
  align-items: center;
  gap: 3px;
  height: 20px;
  flex-shrink: 0;
}

.wave-bar {
  width: 3px;
  height: 100%;
  background: var(--focus-color);
  border-radius: 2px;
  animation: wave 1.2s ease-in-out infinite;
}

.wave-bar:nth-child(1) {
  animation-delay: 0s;
}

.wave-bar:nth-child(2) {
  animation-delay: 0.2s;
}

.wave-bar:nth-child(3) {
  animation-delay: 0.4s;
}

@keyframes wave {
  0%, 100% {
    transform: scaleY(0.3);
    opacity: 0.5;
  }
  50% {
    transform: scaleY(1);
    opacity: 1;
  }
}

/* ===== Cards Zone ===== */
.cards-zone {
  flex: 1;
  min-height: 0;
}

.card-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.task-card {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 10px;
  padding: 12px;
  border-radius: 10px;
  background: var(--ui-well);
  border: 1px solid var(--v2-line);;
  cursor: pointer;
  transition: all 0.2s;
  font-family: inherit;
}

.task-card:hover {
  background: var(--ui-well);
  border-color: var(--v2-muted);
  transform: translateY(-1px);
}

.task-card:active {
  transform: translateY(0);
}

.card-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  border-radius: 8px;
  flex-shrink: 0;
}

.today-icon {
  background: color-mix(in srgb, var(--focus-color) 15%, transparent);
  color: var(--focus-color);
}

.tomorrow-icon {
  background: color-mix(in srgb, #a78bfa 15%, transparent);
  color: #a78bfa;
}

.week-icon {
  background: color-mix(in srgb, #60a5fa 15%, transparent);
  color: #60a5fa;
}

.done-icon {
  background: color-mix(in srgb, #10b981 15%, transparent);
  color: #10b981;
}

.card-content {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  flex: 1;
  min-width: 0;
}

.card-label {
  font-size: 12px;
  color: var(--v2-muted);
  font-weight: 500;
  line-height: 1;
}

.card-count {
  font-size: 20px;
  font-weight: 700;
  color: var(--v2-ink);
  line-height: 1;
}

.task-group-badge {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 10px;
  color: var(--v2-muted);
  background: var(--ui-well);
  padding: 1px 6px;
  border-radius: 4px;
  flex-shrink: 0;
  margin-left: 6px;
}

.group-dot-sm {
  display: inline-block;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}
/* ===== 紧凑快速专注按钮 ===== */
.quick-add-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.quick-add-row .quick-add-input {
  flex: 1;
  min-width: 0;
}

.quick-add-btn {
  flex-shrink: 0;
  height: 34px;
  padding: 0 10px;
  border-radius: 8px;
  border: 1px solid color-mix(in srgb, var(--focus-color) 30%, transparent);
  background: color-mix(in srgb, var(--focus-color) 10%, rgba(255, 255, 255, 0.03));
  color: var(--v2-ink);
  font-size: 11px;
  font-family: inherit;
  cursor: pointer;
  transition: background 0.2s, border-color 0.2s, opacity 0.2s;
}

.quick-add-btn:hover:not(:disabled) {
  background: color-mix(in srgb, var(--focus-color) 18%, rgba(255, 255, 255, 0.04));
  border-color: color-mix(in srgb, var(--focus-color) 45%, transparent);
}

.quick-add-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
</style>
