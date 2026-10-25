import { onUnmounted } from 'vue'
import { emit } from '@tauri-apps/api/event'
import { useTasks, todayString, isTaskDoneOnDate, isRepeatEnded } from './useTasks'
import { useNotifications, addRecord } from './useNotifications'

// 每日任务到点提醒：面板常驻检查，命中 repeatTime 时记录到通知中心并通知岛内 peek。
// 已提醒的 key 存在 localStorage，应用重启也不会在同一分钟重复提醒。

const REMINDED_KEY = 'focus-island-task-reminders'
const CHECK_INTERVAL_MS = 15000

let timer: ReturnType<typeof setInterval> | null = null
let started = false

function loadReminded(): Set<string> {
  try {
    const raw = localStorage.getItem(REMINDED_KEY)
    const parsed = raw ? JSON.parse(raw) : []
    return new Set(
      Array.isArray(parsed) ? parsed.filter((v): v is string => typeof v === 'string') : [],
    )
  } catch {
    return new Set()
  }
}

const reminded = loadReminded()

function markReminded(key: string) {
  reminded.add(key)
  try {
    localStorage.setItem(REMINDED_KEY, JSON.stringify([...reminded].slice(-200)))
  } catch {
    // ignore storage failures
  }
}

function currentHHmm(): string {
  const now = new Date()
  return `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}`
}

export function useTaskReminders() {
  const { tasks } = useTasks()
  useNotifications()

  function check() {
    const hhmm = currentHHmm()
    const today = todayString.value
    for (const task of tasks.value) {
      if (task.repeat !== 'daily') continue
      const time = (task.repeatTime ?? '').trim()
      if (!time || time !== hhmm) continue
      if (isTaskDoneOnDate(task, today)) continue
      if (isRepeatEnded(task, today)) continue
      const key = `${task.id}|${today}|${time}`
      if (reminded.has(key)) continue
      markReminded(key)
      addRecord('task-reminder', `${time} · ${task.title}`)
      emit('task-reminder', { id: task.id, title: task.title, time }).catch(() => {})
    }
  }

  if (!started) {
    started = true
    check()
    timer = setInterval(check, CHECK_INTERVAL_MS)
  }

  onUnmounted(() => {
    if (timer !== null) {
      clearInterval(timer)
      timer = null
      started = false
    }
  })
}
