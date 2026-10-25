import { ref, watch, computed } from 'vue'
import { readTextFile, writeTextFile, mkdir, BaseDirectory } from '@tauri-apps/plugin-fs'
import { emitTo, listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useSettings } from './useSettings'
import { useDailyStats } from './useDailyStats'

// --- Date helpers ---

function toDateStr(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

function parseDateStr(s: string): Date {
  const [y, m, d] = s.split('-').map(Number)
  return new Date(y, m - 1, d)
}

// 当前日期字符串（跨天自动刷新，供“今天”分组与每日任务打卡状态使用）
export const todayString = ref(toDateStr(new Date()))

let todayTicker: ReturnType<typeof setInterval> | null = null
function ensureTodayTicker() {
  if (todayTicker !== null) return
  todayTicker = setInterval(() => {
    const next = toDateStr(new Date())
    if (next !== todayString.value) todayString.value = next
  }, 30_000)
}
ensureTodayTicker()

// --- Types ---

export interface Subtask {
  id: string
  title: string
  completed: boolean
  pomodoroCount: number
}

export type TaskPriority = 0 | 1 | 2 | 3
export type TimeCategory = 'overdue' | 'today' | 'tomorrow' | 'week' | 'later' | 'nodate'
export type TaskRepeat = 'daily'

export interface Task {
  id: string
  title: string
  note: string
  groupId: string | null
  dueDate: string | null
  completed: boolean
  pomodoroCount: number
  priority: TaskPriority
  subtasks: Subtask[]
  createdAt: number
  updatedAt: number
  lastActiveAt: number
  repeat?: TaskRepeat | null
  repeatTime?: string | null
  repeatStart?: string | null
  repeatEnd?: string | null
  completionDates?: string[]
}

export interface RepeatOptions {
  repeat: TaskRepeat
  time?: string | null
  start?: string | null
  end?: string | null
}

interface TaskBuckets {
  active: Task[]
  completed: Task[]
  recentFocus: Task[]
  overdue: Task[]
  today: Task[]
  tomorrow: Task[]
  week: Task[]
  later: Task[]
  nodate: Task[]
  groups: Map<string, Task[]>
}

export function getTaskTimeCategory(task: Task, today = todayString.value): TimeCategory {
  if (task.repeat === 'daily') {
    if (isRepeatEnded(task)) return 'nodate'
    if (task.repeatStart && task.repeatStart > today) return 'later'
    return 'today'
  }
  if (!task.dueDate) return 'nodate'
  const due = parseDateStr(task.dueDate)
  const diff = Math.floor((due.getTime() - parseDateStr(today).getTime()) / (24 * 60 * 60 * 1000))
  if (diff < 0) return 'overdue'
  if (diff === 0) return 'today'
  if (diff === 1) return 'tomorrow'
  if (diff <= 6) return 'week'
  return 'later'
}

// 每日重复任务：完成状态按“今天是否已打卡”计算
export function isTaskDoneOnDate(task: Task, date: string): boolean {
  if (task.repeat === 'daily') {
    return (task.completionDates ?? []).includes(date)
  }
  return task.completed
}

export function isRecurringTask(task: Task): boolean {
  return task.repeat === 'daily'
}

// 重复任务在指定日期是否处于生效区间
export function isRepeatActiveOn(task: Task, date: string): boolean {
  if (task.repeat !== 'daily') return false
  const start = task.repeatStart ?? task.dueDate ?? ''
  if (start && start > date) return false
  if (task.repeatEnd && date > task.repeatEnd) return false
  return true
}

export function isRepeatEnded(task: Task, today = todayString.value): boolean {
  if (task.repeat !== 'daily') return false
  return !!task.repeatEnd && today > task.repeatEnd
}

// 卡片/详情页的重复摘要，例如“每天 08:00 · 至 11/30”
export function formatRepeatLabel(task: Task): string {
  if (task.repeat !== 'daily') return ''
  let label = '每天'
  if (task.repeatTime) label += ` ${task.repeatTime}`
  if (task.repeatEnd) {
    const [, m, d] = task.repeatEnd.split('-').map(Number)
    label += ` · 至 ${m}/${d}`
  } else if (task.repeatStart && task.repeatStart > todayString.value) {
    const [, m, d] = task.repeatStart.split('-').map(Number)
    label += ` · ${m}/${d} 起`
  }
  return label
}

// --- State ---

const TASKS_FILE = 'focus-island/tasks.json'

const tasks = ref<Task[]>([])
const loaded = ref(false)
let loadPromise: Promise<void> | null = null

// --- Persistence ---

async function load() {
  try {
    const raw = await readTextFile(TASKS_FILE, { baseDir: BaseDirectory.AppData })
    const parsed = JSON.parse(raw)
    tasks.value = parsed.map((t: Record<string, unknown> & Partial<Task>) => {
      // Migration: convert old category field to dueDate
      if ('category' in t && !('dueDate' in t)) {
        const cat = (t as Record<string, unknown>).category as string | undefined
        const now = new Date()
        if (cat === 'tomorrow') {
          now.setDate(now.getDate() + 1)
        } else if (cat === 'week') {
          now.setDate(now.getDate() + 2)
        }
        // 'today' or anything else → today's date
        t.dueDate = toDateStr(now)
      }

      return {
        note: t.note ?? '',
        groupId: t.groupId ?? null,
        dueDate: t.dueDate ?? toDateStr(new Date()),
        priority: t.completed ? 0 : ((t.priority ?? 0) as TaskPriority),
        subtasks: t.subtasks ?? [],
        repeat: t.repeat ?? null,
        repeatTime: t.repeatTime ?? null,
        repeatStart: t.repeatStart ?? (t.repeat === 'daily' ? (t.dueDate ?? toDateStr(new Date())) : null),
        repeatEnd: t.repeatEnd ?? null,
        completionDates: t.completionDates ?? [],
        ...t,
        lastActiveAt: t.lastActiveAt ?? 0,
      } as Task
    })
  } catch {
    tasks.value = []
  }
  loaded.value = true
}

function ensureLoaded() {
  if (loaded.value || loadPromise) return
  loadPromise = load().finally(() => {
    loadPromise = null
  })
}

let isSyncing = false

async function save() {
  if (!loaded.value || isSyncing) return
  try {
    await mkdir('focus-island', { baseDir: BaseDirectory.AppData, recursive: true })
    const json = JSON.stringify(tasks.value)
    await writeTextFile(TASKS_FILE, json, { baseDir: BaseDirectory.AppData })
    // Broadcast to other windows
    const self = getCurrentWebviewWindow().label
    const targets = self === 'main' ? ['panel'] : ['main']
    for (const target of targets) {
      emitTo(target, 'tasks-updated', json).catch(() => {})
    }
  } catch (e) {
    console.error('Failed to save tasks', e)
  }
}

watch(tasks, save, { deep: true })

// Listen for cross-window task updates
listen<string>('tasks-updated', (event) => {
  try {
    isSyncing = true
    const parsed = JSON.parse(event.payload) as Task[]
    tasks.value = parsed
    // nextTick to let watch fire and skip save due to isSyncing
    setTimeout(() => { isSyncing = false }, 50)
  } catch { /* ignore */ }
})

// --- Composable ---

export function useTasks() {
  ensureLoaded()
  const { recordTaskCompleted, unrecordTaskCompleted, todayStat } = useDailyStats()

  function addTask(
    title: string,
    dueDate?: string | null,
    groupId?: string | null,
    priority?: TaskPriority,
    repeatOptions?: RepeatOptions | null
  ) {
    const now = Date.now()
    const today = toDateStr(new Date())
    const task: Task = {
      id: crypto.randomUUID(),
      title: title.trim(),
      note: '',
      groupId: groupId ?? null,
      dueDate: repeatOptions ? today : (dueDate ?? today),
      completed: false,
      pomodoroCount: 0,
      priority: priority ?? 0,
      subtasks: [],
      createdAt: now,
      updatedAt: now,
      lastActiveAt: 0,
      repeat: repeatOptions?.repeat ?? null,
      repeatTime: repeatOptions?.time || null,
      repeatStart: repeatOptions ? (repeatOptions.start || today) : null,
      repeatEnd: repeatOptions?.end || null,
      completionDates: [],
    }

    tasks.value.unshift(task)
    return task
  }

  function updateTask(id: string, patch: Partial<Pick<Task, 'title' | 'note' | 'groupId' | 'dueDate' | 'completed' | 'priority' | 'repeat' | 'repeatTime' | 'repeatStart' | 'repeatEnd'>>) {
    const task = tasks.value.find(t => t.id === id)
    if (!task) return

    const wasCompleted = task.completed
    Object.assign(task, patch)

    if (task.completed) {
      task.priority = 0
    }
    if (!wasCompleted && task.completed) {
      recordTaskCompleted(task.title)
    }

    task.updatedAt = Date.now()
  }

  // 切换/取消每日重复
  function setTaskRepeat(id: string, options: RepeatOptions | null) {
    const task = tasks.value.find(t => t.id === id)
    if (!task) return

    const today = toDateStr(new Date())
    task.repeat = options?.repeat ?? null
    task.repeatTime = options?.time || null
    task.repeatStart = options ? (options.start || today) : null
    task.repeatEnd = options?.end || null
    task.completionDates = task.completionDates ?? []

    if (options) {
      task.completed = false
      task.priority = 0
      task.dueDate = today
    }

    task.updatedAt = Date.now()
  }

  function setTaskPriority(id: string, priority: TaskPriority) {
    const task = tasks.value.find(t => t.id === id)
    if (!task || task.completed) return

    task.priority = priority
    task.updatedAt = Date.now()
  }

  function deleteTask(id: string) {
    tasks.value = tasks.value.filter(t => t.id !== id)
  }

  function toggleComplete(id: string) {
    const task = tasks.value.find(t => t.id === id)
    if (!task) return

    // 每日重复任务：勾选记录当天完成，取消勾选撤销当天记录
    if (task.repeat === 'daily') {
      if (isRepeatEnded(task)) return
      const today = toDateStr(new Date())
      const dates = task.completionDates ?? (task.completionDates = [])
      const idx = dates.indexOf(today)
      if (idx >= 0) {
        dates.splice(idx, 1)
        unrecordTaskCompleted(task.title, today)
      } else {
        dates.push(today)
        recordTaskCompleted(task.title)
      }
      task.updatedAt = Date.now()
      return
    }

    if (task.completed) {
      // 取消完成：按最后一次完成日期回退统计
      const dates = task.completionDates ?? []
      const completedDate = dates.length
        ? dates[dates.length - 1]
        : toDateStr(new Date(task.updatedAt))
      if (dates.length) task.completionDates = dates.slice(0, -1)
      task.completed = false
      task.priority = 1 as TaskPriority
      unrecordTaskCompleted(task.title, completedDate)
    } else {
      task.completed = true
      task.priority = 0
      const today = toDateStr(new Date())
      task.completionDates = [...(task.completionDates ?? []), today]
      recordTaskCompleted(task.title)
    }
    task.updatedAt = Date.now()
  }

  function incrementPomodoro(id: string) {
    const task = tasks.value.find(t => t.id === id)
    if (task) {
      task.pomodoroCount++
      task.updatedAt = Date.now()
    }
  }

  function touchTask(id: string) {
    const task = tasks.value.find(t => t.id === id)
    if (task) {
      task.lastActiveAt = Date.now()
    }
  }

  // --- Subtask CRUD ---

  function addSubtask(taskId: string, title: string): void {
    const task = tasks.value.find(t => t.id === taskId)
    if (!task) return
    task.subtasks.push({
      id: crypto.randomUUID(),
      title: title.trim(),
      completed: false,
      pomodoroCount: 0,
    })
    task.updatedAt = Date.now()
  }

  function toggleSubtask(taskId: string, subtaskId: string): void {
    const task = tasks.value.find(t => t.id === taskId)
    if (!task) return
    const subtask = task.subtasks.find(s => s.id === subtaskId)
    if (!subtask) return
    subtask.completed = !subtask.completed
    task.updatedAt = Date.now()
  }

  function deleteSubtask(taskId: string, subtaskId: string): void {
    const task = tasks.value.find(t => t.id === taskId)
    if (!task) return
    task.subtasks = task.subtasks.filter(s => s.id !== subtaskId)
    task.updatedAt = Date.now()
  }

  function incrementSubtaskPomodoro(taskId: string, subtaskId: string): void {
    const task = tasks.value.find(t => t.id === taskId)
    if (!task) return
    const subtask = task.subtasks.find(s => s.id === subtaskId)
    if (!subtask) return
    subtask.pomodoroCount++
    task.updatedAt = Date.now()
  }

  // --- Computed views ---

  const taskBuckets = computed<TaskBuckets>(() => {
    const buckets: TaskBuckets = {
      active: [],
      completed: [],
      recentFocus: [],
      overdue: [],
      today: [],
      tomorrow: [],
      week: [],
      later: [],
      nodate: [],
      groups: new Map(),
    }

    for (const task of tasks.value) {
      // 每日重复任务：按生效区间归入“今天 / 稍后 / 已完成（已结束）”
      if (task.repeat === 'daily') {
        if (isRepeatEnded(task)) {
          buckets.completed.push(task)
          continue
        }

        buckets.active.push(task)
        if (task.lastActiveAt > 0) {
          buckets.recentFocus.push(task)
        }
        if (task.groupId) {
          const groupTasks = buckets.groups.get(task.groupId)
          if (groupTasks) {
            groupTasks.push(task)
          } else {
            buckets.groups.set(task.groupId, [task])
          }
        }

        if (task.repeatStart && task.repeatStart > todayString.value) {
          buckets.later.push(task)
        } else {
          buckets.today.push(task)
        }
        continue
      }

      if (task.completed) {
        buckets.completed.push(task)
        continue
      }

      buckets.active.push(task)

      if (task.lastActiveAt > 0) {
        buckets.recentFocus.push(task)
      }

      if (task.groupId) {
        const groupTasks = buckets.groups.get(task.groupId)
        if (groupTasks) {
          groupTasks.push(task)
        } else {
          buckets.groups.set(task.groupId, [task])
        }
      }

      const category = getTaskTimeCategory(task)
      if (category === 'overdue') {
        buckets.overdue.push(task)
        buckets.today.push(task)
      } else if (category === 'today') {
        buckets.today.push(task)
      } else if (category === 'tomorrow') {
        buckets.tomorrow.push(task)
      } else if (category === 'week') {
        buckets.week.push(task)
      } else if (category === 'later') {
        buckets.later.push(task)
      } else {
        buckets.nodate.push(task)
      }
    }

    buckets.recentFocus.sort((a, b) => b.lastActiveAt - a.lastActiveAt)
    buckets.recentFocus = buckets.recentFocus.slice(0, 3)
    return buckets
  })

  const activeTasks = computed(() => taskBuckets.value.active)
  const completedTasks = computed(() => taskBuckets.value.completed)
  const recentFocusTasks = computed(() => taskBuckets.value.recentFocus)

  const overdueTasks = computed(() => taskBuckets.value.overdue)
  const todayTasks = computed(() => taskBuckets.value.today)
  const tomorrowTasks = computed(() => taskBuckets.value.tomorrow)
  const weekTasks = computed(() => taskBuckets.value.week)
  const laterTasks = computed(() => taskBuckets.value.later)
  const nodateTasks = computed(() => taskBuckets.value.nodate)

  function groupTasks(groupId: string): Task[] {
    return taskBuckets.value.groups.get(groupId) ?? []
  }

  const todayStats = computed(() => {
    const { settings } = useSettings()
    const totalPomodoros = tasks.value.reduce((sum, t) => sum + t.pomodoroCount, 0)
    const focusMinutes = totalPomodoros * settings.value.focusDuration
    return { totalPomodoros, focusMinutes, completedToday: todayStat.value.tasksCompleted }
  })

  return {
    tasks,
    activeTasks,
    completedTasks,
    recentFocusTasks,
    overdueTasks,
    todayTasks,
    tomorrowTasks,
    weekTasks,
    laterTasks,
    nodateTasks,
    todayStats,
    addTask,
    updateTask,
    setTaskRepeat,
    setTaskPriority,
    deleteTask,
    toggleComplete,
    incrementPomodoro,
    touchTask,
    addSubtask,
    toggleSubtask,
    deleteSubtask,
    incrementSubtaskPomodoro,
    groupTasks,
  }
}
