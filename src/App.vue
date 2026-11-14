<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import Island from './components/Island.vue'
import PanelApp from './panel/PanelApp.vue'

const windowLabel = ref<string>('main')

onMounted(async () => {
  try {
    const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow')
    windowLabel.value = getCurrentWebviewWindow().label
  } catch {
    windowLabel.value = 'main'
  }
  // 窗口显示后再强刷一次：部分 Windows/WebView2 组合会在 show 之后重新加回 WS_CAPTION
  void invoke('force_current_window_frameless').catch(() => {})
})
</script>

<template>
  <Island v-if="windowLabel === 'main'" />
  <PanelApp v-else-if="windowLabel === 'panel'" />
</template>
