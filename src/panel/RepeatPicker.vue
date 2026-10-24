<script setup lang="ts">
import { computed, ref } from 'vue'
import type { RepeatOptions } from '../composables/useTasks'

const props = withDefaults(
  defineProps<{
    repeat?: 'daily' | null
    time?: string | null
    start?: string | null
    end?: string | null
  }>(),
  { repeat: null, time: null, start: null, end: null }
)

const emit = defineEmits<{
  apply: [options: RepeatOptions | null]
  close: []
}>()

function toDateStr(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

const todayStr = toDateStr(new Date())

type Mode = 'none' | 'daily' | 'range'

const initialMode: Mode =
  props.repeat === 'daily'
    ? (props.start && props.start > todayStr && props.end ? 'range' : (props.end ? 'daily' : 'daily'))
    : 'none'

const mode = ref<Mode>(initialMode)
const timeValue = ref(props.time ?? '')
const endDate = ref<string | null>(props.end ?? null)
const rangeStart = ref<string | null>(props.start ?? todayStr)
const rangeEnd = ref<string | null>(props.end ?? null)

const initialView = new Date()
if (props.start) {
  const [y, m] = props.start.split('-').map(Number)
  initialView.setFullYear(y, m - 1, 1)
}
const viewYear = ref(initialView.getFullYear())
const viewMonth = ref(initialView.getMonth())

const calendarCells = computed(() => {
  const year = viewYear.value
  const month = viewMonth.value
  const first = new Date(year, month, 1)
  const padding = first.getDay() === 0 ? 6 : first.getDay() - 1
  const daysInMonth = new Date(year, month + 1, 0).getDate()
  const prevDays = new Date(year, month, 0).getDate()

  type Cell = { date: string; day: number; current: boolean }
  const cells: Cell[] = []

  for (let i = padding - 1; i >= 0; i--) {
    const d = prevDays - i
    const date = new Date(year, month - 1, d)
    cells.push({ date: toDateStr(date), day: d, current: false })
  }
  for (let d = 1; d <= daysInMonth; d++) {
    cells.push({ date: toDateStr(new Date(year, month, d)), day: d, current: true })
  }
  let nextDay = 1
  while (cells.length < 42) {
    cells.push({ date: toDateStr(new Date(year, month + 1, nextDay)), day: nextDay, current: false })
    nextDay++
  }
  return cells
})

const weekLabels = ['一', '二', '三', '四', '五', '六', '日']
const monthTitle = computed(() => `${viewYear.value}年${viewMonth.value + 1}月`)

function prevMonth() {
  if (viewMonth.value === 0) {
    viewMonth.value = 11
    viewYear.value--
  } else {
    viewMonth.value--
  }
}

function nextMonth() {
  if (viewMonth.value === 11) {
    viewMonth.value = 0
    viewYear.value++
  } else {
    viewMonth.value++
  }
}

function setMode(next: Mode) {
  mode.value = next
  if (next === 'range' && !rangeStart.value) {
    rangeStart.value = todayStr
    rangeEnd.value = null
  }
}

function onPickDate(date: string) {
  if (mode.value === 'none') return

  if (mode.value === 'daily') {
    endDate.value = endDate.value === date ? null : date
    return
  }

  if (!rangeStart.value || (rangeStart.value && rangeEnd.value)) {
    rangeStart.value = date
    rangeEnd.value = null
  } else if (date < rangeStart.value) {
    rangeStart.value = date
  } else if (date === rangeStart.value) {
    rangeEnd.value = date
  } else {
    rangeEnd.value = date
  }
}

function isSelected(date: string): boolean {
  if (mode.value === 'daily') return endDate.value === date
  if (mode.value === 'range') return rangeStart.value === date || rangeEnd.value === date
  return false
}

function isInRange(date: string): boolean {
  if (mode.value !== 'range' || !rangeStart.value || !rangeEnd.value) return false
  return date > rangeStart.value && date < rangeEnd.value
}

function formatMd(date: string): string {
  const [, m, d] = date.split('-').map(Number)
  return `${m}/${d}`
}

const summary = computed(() => {
  if (mode.value === 'none') return '不重复'
  if (mode.value === 'daily') return endDate.value ? `每天，至 ${formatMd(endDate.value)}` : '每天，不限结束'
  if (!rangeStart.value) return '选择开始日期'
  if (!rangeEnd.value) return `${formatMd(rangeStart.value)} 起，选结束日期`
  return `${formatMd(rangeStart.value)} - ${formatMd(rangeEnd.value)}`
})

function confirm() {
  if (mode.value === 'none') {
    emit('apply', null)
    emit('close')
    return
  }

  if (mode.value === 'daily') {
    emit('apply', {
      repeat: 'daily',
      time: timeValue.value || null,
      start: todayStr,
      end: endDate.value,
    })
    emit('close')
    return
  }

  const start = rangeStart.value ?? todayStr
  const end = rangeEnd.value ?? start
  emit('apply', {
    repeat: 'daily',
    time: timeValue.value || null,
    start,
    end,
  })
  emit('close')
}
</script>

<template>
  <div class="repeat-picker" @click.stop>
    <div class="mode-tabs">
      <button :class="['mode-tab', { active: mode === 'none' }]" @click="setMode('none')">不重复</button>
      <button :class="['mode-tab', { active: mode === 'daily' }]" @click="setMode('daily')">每天</button>
      <button :class="['mode-tab', { active: mode === 'range' }]" @click="setMode('range')">自定义范围</button>
    </div>

    <div v-if="mode !== 'none'" class="picker-body">
      <div class="month-header">
        <button class="nav-btn" @click="prevMonth" title="上个月">&lt;</button>
        <span class="month-title">{{ monthTitle }}</span>
        <button class="nav-btn" @click="nextMonth" title="下个月">&gt;</button>
      </div>

      <div class="weekdays">
        <span v-for="w in weekLabels" :key="w">{{ w }}</span>
      </div>

      <div class="days-grid">
        <button
          v-for="cell in calendarCells"
          :key="cell.date"
          :class="[
            'day-cell',
            {
              muted: !cell.current,
              today: cell.date === todayStr,
              selected: isSelected(cell.date),
              'in-range': isInRange(cell.date),
            },
          ]"
          @click="onPickDate(cell.date)"
        >
          {{ cell.day }}
        </button>
      </div>

      <div class="picker-summary">
        <span class="summary-dot"></span>
        {{ summary }}
      </div>

      <div class="time-row">
        <span class="time-label">定时</span>
        <input v-model="timeValue" type="time" class="time-input" />
        <button v-if="timeValue" class="time-clear" @click="timeValue = ''">清除</button>
      </div>
    </div>

    <div class="picker-footer">
      <button class="footer-btn" @click="emit('close')">取消</button>
      <button class="footer-btn primary" @click="confirm">确定</button>
    </div>
  </div>
</template>

<style scoped>
@reference "../styles.css";

.repeat-picker {
  width: 264px;
  padding: 10px;
  color: rgba(255, 255, 255, 0.9);
  font-family: inherit;
  user-select: none;
}

.mode-tabs {
  display: flex;
  gap: 4px;
  padding: 2px;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.04);
}

.mode-tab {
  flex: 1;
  padding: 5px 4px;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: rgba(255, 255, 255, 0.5);
  font-size: 11px;
  font-family: inherit;
  cursor: pointer;
  transition: all 0.15s;
}

.mode-tab:hover {
  color: rgba(255, 255, 255, 0.8);
}

.mode-tab.active {
  background: color-mix(in srgb, var(--focus-color) 16%, transparent);
  color: color-mix(in srgb, var(--focus-color) 85%, #fff);
}

.picker-body {
  margin-top: 8px;
}

.month-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 2px;
  margin-bottom: 6px;
}

.month-title {
  font-size: 12px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.85);
}

.nav-btn {
  width: 22px;
  height: 22px;
  border-radius: 6px;
  border: none;
  background: transparent;
  color: rgba(255, 255, 255, 0.45);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}

.nav-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  color: #fff;
}

.weekdays {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  text-align: center;
  font-size: 10px;
  color: rgba(255, 255, 255, 0.3);
  margin-bottom: 2px;
}

.days-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 2px;
}

.day-cell {
  height: 28px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: rgba(255, 255, 255, 0.8);
  font-size: 11px;
  font-family: inherit;
  cursor: pointer;
  transition: all 0.12s;
}

.day-cell:hover {
  background: rgba(255, 255, 255, 0.08);
}

.day-cell.muted {
  color: rgba(255, 255, 255, 0.2);
}

.day-cell.today {
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--focus-color) 45%, transparent);
}

.day-cell.in-range {
  background: color-mix(in srgb, var(--focus-color) 9%, transparent);
  border-radius: 0;
}

.day-cell.selected {
  background: var(--focus-color);
  color: #fff;
  font-weight: 700;
}

.picker-summary {
  display: flex;
  align-items: center;
  gap: 5px;
  margin-top: 7px;
  padding: 5px 7px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.04);
  font-size: 11px;
  color: rgba(255, 255, 255, 0.65);
}

.summary-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--focus-color);
  flex-shrink: 0;
}

.time-row {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 7px;
}

.time-label {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.4);
  flex-shrink: 0;
}

.time-input {
  flex: 1;
  min-width: 0;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 6px;
  padding: 3px 6px;
  font-size: 11px;
  font-family: inherit;
  color: rgba(255, 255, 255, 0.85);
  color-scheme: dark;
  outline: none;
}

.time-clear {
  border: none;
  background: none;
  color: rgba(255, 255, 255, 0.4);
  font-size: 11px;
  font-family: inherit;
  cursor: pointer;
  padding: 2px 4px;
}

.time-clear:hover {
  color: #fff;
}

.picker-footer {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
  margin-top: 9px;
  padding-top: 8px;
  border-top: 1px solid rgba(255, 255, 255, 0.07);
}

.footer-btn {
  padding: 5px 14px;
  border-radius: 7px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  background: rgba(255, 255, 255, 0.04);
  color: rgba(255, 255, 255, 0.65);
  font-size: 11px;
  font-family: inherit;
  cursor: pointer;
  transition: all 0.15s;
}

.footer-btn:hover {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}

.footer-btn.primary {
  border-color: color-mix(in srgb, var(--focus-color) 40%, transparent);
  background: color-mix(in srgb, var(--focus-color) 18%, transparent);
  color: color-mix(in srgb, var(--focus-color) 85%, #fff);
}

.footer-btn.primary:hover {
  background: color-mix(in srgb, var(--focus-color) 28%, transparent);
}
</style>
