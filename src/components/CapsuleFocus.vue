<script setup lang="ts">
import { computed } from 'vue'
import { useTimer } from '../composables/useTimer'
import { useTasks } from '../composables/useTasks'
import { PIXEL_3X5, layoutPixelText } from '../utils/pixelFont'

const { displayTime, phase, activeTaskId, activeTaskTitle, activeSubtaskTitle, running } = useTimer()
const { tasks } = useTasks()

defineProps<{ radioPlaying?: boolean }>()

const activeTask = computed(() =>
  tasks.value.find(t => t.id === activeTaskId.value)
)
const resolvedTitle = computed(() => {
  if (activeSubtaskTitle.value) return activeSubtaskTitle.value
  return activeTaskTitle.value ?? activeTask.value?.title ?? null
})

const phaseColor = computed(() =>
  phase.value === 'focus' ? 'var(--focus-color)' : 'var(--break-color)'
)

// 固定 3px：与 idle 时钟保持一致，整体再小一号
const pixelUnit = 3
const pixelTime = computed(() => layoutPixelText(displayTime.value, PIXEL_3X5, pixelUnit, 1))
</script>

<template>
  <div class="capsule-focus flex items-center justify-between w-full h-full px-4 gap-3">
    <!-- Phase indicator dot -->
    <div
      class="shrink-0 w-2 h-2 rounded-full"
      :class="running ? 'animate-pulse' : ''"
      :style="{ backgroundColor: phaseColor, width: `calc(8px * var(--island-scale, 1))`, height: `calc(8px * var(--island-scale, 1))` }"
    />

    <!-- Task name -->
    <div class="flex-1 min-w-0">
      <span
        class="font-medium truncate block text-[var(--capsule-text)]"
        :style="{ fontSize: `calc(12px * var(--island-scale, 1))` }"
      >{{ resolvedTitle ?? (phase === 'break' ? '休息一下' : '专注中') }}</span>
    </div>

    <!-- Countdown -->
    <div
      class="shrink-0 flex items-center"
      :style="{ gap: `calc(8px * var(--island-scale, 1))` }"
    >
      <svg
        class="capsule-pixel-time"
        :width="pixelTime.width"
        :height="pixelTime.height"
        :viewBox="`0 0 ${pixelTime.width} ${pixelTime.height}`"
        shape-rendering="crispEdges"
        :style="{ '--pixel-color': phaseColor }"
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
      <span class="text-[var(--capsule-muted)]" :style="{ fontSize: `calc(10px * var(--island-scale, 1))` }">
        {{ running ? '' : '⏸' }}
      </span>
      <div v-if="radioPlaying" class="wave-bars">
        <span class="wave-bar" />
        <span class="wave-bar" />
        <span class="wave-bar" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.capsule-pixel-time {
  flex-shrink: 0;
  display: block;
  image-rendering: pixelated;
}

.capsule-pixel-time rect {
  fill: var(--pixel-color);
}

.wave-bars {
  display: flex;
  align-items: flex-end;
  gap: calc(4px * var(--island-scale, 1));
  height: calc(12px * var(--island-scale, 1));
}

.wave-bar {
  width: calc(4px * var(--island-scale, 1));
  height: calc(12px * var(--island-scale, 1));
  background: var(--focus-color);
  border-radius: 0;
  opacity: 0.8;
  animation: wave-bounce 0.8s steps(2, end) infinite;
}

.wave-bar:nth-child(1) {
  animation-delay: 0s;
  clip-path: inset(30% 0 0);
}

.wave-bar:nth-child(2) {
  animation-delay: 0.15s;
  clip-path: inset(0);
}

.wave-bar:nth-child(3) {
  animation-delay: 0.3s;
  clip-path: inset(55% 0 0);
}

@keyframes wave-bounce {
  0%, 100% { clip-path: inset(70% 0 0); }
  50% { clip-path: inset(0); }
}
</style>
