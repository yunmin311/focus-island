import { ref, watch, computed } from 'vue'
import { readTextFile, writeTextFile, mkdir, BaseDirectory } from '@tauri-apps/plugin-fs'
import { emitTo, listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

export interface DailyStat {
  date: string        // "2026-04-18"
  pomodoros: number   // 当日番茄数
  focusMinutes: number // 当日专注分钟
  tasksCompleted: number // 当日完成任务数
  completedTitles?: string[] // 当日完成的任务标题（用于统计明细）
}

const STATS_FILE = 'focus-island/daily-stats.json'

const dailyStats = ref<DailyStat[]>([])
const loaded = ref(false)
let loadPromise: Promise<void> | null = null

function getDateString(date: Date = new Date()): string {
  const y = date.getFullYear()
  const m = String(date.getMonth() + 1).padStart(2, '0')
  const d = String(date.getDate()).padStart(2, '0')
  return `${y}-${m}-${d}`
}

function getStatByDate(date: string): DailyStat | undefined {
  return dailyStats.value.find(s => s.date === date)
}

async function load() {
  try {
    const raw = await readTextFile(STATS_FILE, { baseDir: BaseDirectory.AppData })
    dailyStats.value = JSON.parse(raw)
  } catch {
    dailyStats.value = []
  }
  loaded.value = true
}

function ensureLoaded() {
  if (loaded.value || loadPromise) return
  loadPromise = load().finally(() => {
    loadPromise = null
  })
}

async function save() {
  if (!loaded.value || isSyncing) return
  try {
    await mkdir('focus-island', { baseDir: BaseDirectory.AppData, recursive: true })
    const json = JSON.stringify(dailyStats.value)
    await writeTextFile(STATS_FILE, json, { baseDir: BaseDirectory.AppData })
    // 同步到另一个窗口（岛 / 面板）
    const self = getCurrentWebviewWindow().label
    const targets = self === 'main' ? ['panel'] : ['main']
    for (const target of targets) {
      emitTo(target, 'daily-stats-updated', json).catch(() => {})
    }
  } catch (e) {
    console.error('Failed to save daily stats', e)
  }
}

let isSyncing = false

watch(dailyStats, save, { deep: true })

listen<string>('daily-stats-updated', (event) => {
  try {
    isSyncing = true
    dailyStats.value = JSON.parse(event.payload)
    setTimeout(() => { isSyncing = false }, 50)
  } catch { /* ignore */ }
})

export function useDailyStats() {
  ensureLoaded()

  function recordPomodoro(focusMinutes: number) {
    const today = getDateString()
    let stat = getStatByDate(today)
    if (!stat) {
      stat = { date: today, pomodoros: 0, focusMinutes: 0, tasksCompleted: 0 }
      dailyStats.value.push(stat)
    }
    stat.pomodoros++
    stat.focusMinutes += focusMinutes
  }

  function recordTaskCompleted(title?: string) {
    const today = getDateString()
    let stat = getStatByDate(today)
    if (!stat) {
      stat = { date: today, pomodoros: 0, focusMinutes: 0, tasksCompleted: 0, completedTitles: [] }
      dailyStats.value.push(stat)
    }
    stat.tasksCompleted++
    if (title) {
      if (!stat.completedTitles) stat.completedTitles = []
      stat.completedTitles.push(title)
    }
  }

  // 取消勾选时回退当天的完成记录；只在当天明细里确实记过该标题时才回退，
  // 避免影响历史统计（旧版本没有 completedTitles 的日期不会被误减）。
  function unrecordTaskCompleted(title?: string, date?: string) {
    const targetDate = date ?? getDateString()
    const stat = getStatByDate(targetDate)
    if (!stat || !stat.completedTitles) return
    const idx = title ? stat.completedTitles.lastIndexOf(title) : -1
    if (idx < 0) return
    stat.completedTitles.splice(idx, 1)
    if (stat.tasksCompleted > 0) stat.tasksCompleted--
    // 回退后当天没有任何数据时移除空记录，保持统计文件干净
    if (
      stat.tasksCompleted === 0 &&
      stat.pomodoros === 0 &&
      stat.focusMinutes === 0 &&
      stat.completedTitles.length === 0
    ) {
      dailyStats.value = dailyStats.value.filter(s => s !== stat)
    }
  }

  function getStatsInRange(startDate: Date, endDate: Date): DailyStat[] {
    const start = getDateString(startDate)
    const end = getDateString(endDate)
    return dailyStats.value.filter(s => s.date >= start && s.date <= end)
  }

  function getMonthStats(year: number, month: number): DailyStat[] {
    const start = new Date(year, month, 1)
    const end = new Date(year, month + 1, 0)
    return getStatsInRange(start, end)
  }

  const todayStat = computed(() => {
    const today = getDateString()
    return getStatByDate(today) ?? { date: today, pomodoros: 0, focusMinutes: 0, tasksCompleted: 0 }
  })

  const weekStats = computed(() => {
    const now = new Date()
    const weekStart = new Date(now)
    weekStart.setDate(now.getDate() - now.getDay())
    weekStart.setHours(0, 0, 0, 0)
    return getStatsInRange(weekStart, now)
  })

  const monthStats = computed(() => {
    const now = new Date()
    return getMonthStats(now.getFullYear(), now.getMonth())
  })

  const allTimeStats = computed(() => {
    const total = { pomodoros: 0, focusMinutes: 0, tasksCompleted: 0 }
    for (const stat of dailyStats.value) {
      total.pomodoros += stat.pomodoros
      total.focusMinutes += stat.focusMinutes
      total.tasksCompleted += stat.tasksCompleted
    }
    return total
  })

  function summarizeStats(stats: DailyStat[]) {
    return stats.reduce(
      (acc, s) => ({
        pomodoros: acc.pomodoros + s.pomodoros,
        focusMinutes: acc.focusMinutes + s.focusMinutes,
        tasksCompleted: acc.tasksCompleted + s.tasksCompleted,
      }),
      { pomodoros: 0, focusMinutes: 0, tasksCompleted: 0 }
    )
  }

  return {
    dailyStats,
    todayStat,
    weekStats,
    monthStats,
    allTimeStats,
    recordPomodoro,
    recordTaskCompleted,
    unrecordTaskCompleted,
    getStatsInRange,
    getMonthStats,
    summarizeStats,
    getDateString,
  }
}
