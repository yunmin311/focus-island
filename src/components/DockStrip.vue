<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  progress: number
  color: string
  scale: number
}>()

const fillWidth = computed(() => `${Math.max(0, Math.min(1, props.progress)) * 100}%`)
const barHeight = computed(() => `${Math.max(2, 3 * props.scale)}px`)
</script>

<template>
  <div class="dock-strip" :style="{ '--strip-color': color }">
    <div class="track" :style="{ height: barHeight }">
      <div class="fill" :style="{ width: fillWidth, height: barHeight, background: color }" />
    </div>
  </div>
</template>

<style scoped>
.dock-strip {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.track {
  width: 60%;
  border-radius: 0;
  background: color-mix(in srgb, var(--strip-color) 25%, transparent);
  overflow: hidden;
  display: flex;
  align-items: stretch;
}

.fill {
  border-radius: 0;
  transition: width 0.4s linear, background 0.3s ease;
  min-width: 0;
}
</style>
