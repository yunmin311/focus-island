<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  data: { key: string; app: string; title: string; body: string } | null
  scale: number
}>()

const emit = defineEmits<{ open: []; hold: []; resume: [] }>()

const appName = computed(() => props.data?.app?.trim() ?? '')
const title = computed(() => {
  const t = props.data?.title?.trim() ?? ''
  return t || appName.value || '系统通知'
})
const sub = computed(() => {
  const b = props.data?.body?.trim() ?? ''
  if (b) return b
  return appName.value && appName.value !== title.value ? appName.value : '来自系统通知中心'
})
const initial = computed(() => (appName.value ? Array.from(appName.value)[0] : ''))
</script>

<template>
  <transition name="peek">
    <div
      v-if="data"
      class="peek-banner"
      :style="{ '--peek-scale': scale }"
      role="button"
      title="打开通知中心"
      @click="emit('open')"
      @mouseenter="emit('hold')"
      @mouseleave="emit('resume')"
    >
      <div class="peek-icon">
        <span v-if="initial" class="peek-initial">{{ initial }}</span>
        <svg
          v-else
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
          <path d="M13.73 21a2 2 0 0 1-3.46 0" />
        </svg>
      </div>
      <div class="peek-text">
        <div class="peek-line">
          <span v-if="appName && appName !== title" class="peek-app">{{ appName }}</span>
          <span class="peek-title">{{ title }}</span>
        </div>
        <div class="peek-sub">{{ sub }}</div>
      </div>
      <svg
        class="peek-chevron"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2.2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <path d="M9 18l6-6-6-6" />
      </svg>
    </div>
  </transition>
</template>

<style scoped>
@reference "../styles.css";

.peek-banner {
  width: calc(392px * var(--peek-scale, 1));
  height: calc(62px * var(--peek-scale, 1));
  border-radius: calc(20px * var(--peek-scale, 1));
  background: var(--notif-bg);
  border: 1px solid var(--notif-border);
  box-shadow: var(--notif-shadow);
  display: flex;
  align-items: center;
  gap: calc(10px * var(--peek-scale, 1));
  padding: 0 calc(14px * var(--peek-scale, 1));
  pointer-events: auto;
  cursor: pointer;
  contain: layout style paint;
  transition: filter 0.18s ease;
}

.peek-banner:hover {
  filter: brightness(1.08);
}

.peek-icon {
  width: calc(32px * var(--peek-scale, 1));
  height: calc(32px * var(--peek-scale, 1));
  border-radius: 50%;
  background: var(--notif-icon-bg);
  color: var(--notif-title-color);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.peek-icon svg {
  width: calc(17px * var(--peek-scale, 1));
  height: calc(17px * var(--peek-scale, 1));
}

.peek-initial {
  font-size: calc(14px * var(--peek-scale, 1));
  font-weight: 700;
  line-height: 1;
}

.peek-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: calc(3px * var(--peek-scale, 1));
}

.peek-line {
  display: flex;
  align-items: center;
  gap: calc(7px * var(--peek-scale, 1));
  min-width: 0;
}

.peek-app {
  flex-shrink: 0;
  max-width: 45%;
  font-size: calc(10px * var(--peek-scale, 1));
  color: var(--notif-sub-color);
  background: var(--notif-icon-bg);
  padding: calc(2px * var(--peek-scale, 1)) calc(6px * var(--peek-scale, 1));
  border-radius: calc(4px * var(--peek-scale, 1));
  line-height: 1.4;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.peek-title {
  font-size: calc(14px * var(--peek-scale, 1));
  font-weight: 700;
  color: var(--notif-title-color);
  line-height: 1.2;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.peek-sub {
  font-size: calc(12px * var(--peek-scale, 1));
  color: var(--notif-sub-color);
  line-height: 1.2;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.peek-chevron {
  width: calc(13px * var(--peek-scale, 1));
  height: calc(13px * var(--peek-scale, 1));
  color: var(--notif-sub-color);
  opacity: 0.5;
  flex-shrink: 0;
}

.peek-enter-active {
  animation: peek-in 240ms ease-out both;
}

.peek-leave-active {
  animation: peek-out 180ms ease-in both;
}

@keyframes peek-in {
  from {
    opacity: 0;
    transform: scale(0.88);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

@keyframes peek-out {
  from {
    opacity: 1;
    transform: scale(1);
  }
  to {
    opacity: 0;
    transform: scale(0.92);
  }
}
</style>
