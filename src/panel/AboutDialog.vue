<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-shell'
import { getName, getVersion } from '@tauri-apps/api/app'
import appIcon from '../../src-tauri/icons/128x128.png'

const emit = defineEmits<{ close: [] }>()

const appName = ref('专注岛')
const appVersion = ref('')

// 定制版：指向自己的 fork，并标注原版来源
const githubUrl = 'https://github.com/yunmin311/focus-island'
const upstreamUrl = 'https://github.com/ipfred/focus-island'

const forkChanges = [
  '刘海停靠 / 完全隐藏 / 点击刘海开面板',
  '电台导入：直链、M3U/PLS、B 站音频',
  '直播断流自动重连 + 备用镜像',
  '通知中心、系统音量与静音',
  '本地音乐、独立透明度、标语编辑',
  '已禁用应用内更新（防止被官方版覆盖）',
]

const versionLabel = computed(() => (appVersion.value ? `v${appVersion.value} · 定制版` : '定制版'))

onMounted(async () => {
  try {
    appName.value = await getName()
    appVersion.value = await getVersion()
  } catch {
    appName.value = '专注岛'
    appVersion.value = '1.3.3'
  }
})

function openGitHub() {
  open(githubUrl)
}

function openUpstream() {
  open(upstreamUrl)
}

function openChanges() {
  open(githubUrl)
}

function onBackdropClick(event: MouseEvent) {
  if (event.target === event.currentTarget) {
    emit('close')
  }
}
</script>

<template>
  <div class="dialog-backdrop" @click="onBackdropClick">
    <div class="dialog-container" @click.stop>
      <!-- 应用图标 -->
      <div class="app-icon-wrapper">
        <img :src="appIcon" :alt="appName" class="app-icon" />
      </div>

      <!-- 应用名称和版本 -->
      <div class="app-info">
        <h2 class="app-name">{{ appName }}</h2>
        <span class="app-version">{{ versionLabel }}</span>
        <span class="app-origin">
          基于
          <button class="inline-link" @click="openUpstream">ipfred/focus-island</button>
        </span>
      </div>

      <!-- 本版改动 -->
      <div class="fork-changes">
        <div class="changes-title">本版改动</div>
        <ul class="changes-list">
          <li v-for="item in forkChanges" :key="item">{{ item }}</li>
        </ul>
      </div>

      <!-- 链接 -->
      <div class="links-row">
        <button class="link-btn" @click="openGitHub">
          <svg class="icon" viewBox="0 0 24 24" fill="currentColor">
            <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/>
          </svg>
          <span>GitHub</span>
        </button>
        <button class="link-btn ghost" @click="openChanges">
          <span>完整说明</span>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
@reference "../styles.css";

.dialog-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  animation: fade-in 0.2s ease-out;
}

@keyframes fade-in {
  from { opacity: 0; }
  to { opacity: 1; }
}

.dialog-container {
  width: 280px;
  padding: 24px 22px;
  background: linear-gradient(180deg, rgba(28, 28, 33, 0.90), rgba(21, 21, 26, 0.88));
  backdrop-filter: blur(28px) saturate(1.3);
  border-radius: 16px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
  animation: scale-in 0.25s cubic-bezier(0.2, 0.8, 0.22, 1);
}

@keyframes scale-in {
  from {
    opacity: 0;
    transform: scale(0.9);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

.app-icon-wrapper {
  width: 72px;
  height: 72px;
  border-radius: 16px;
  overflow: hidden;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
}

.app-icon {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.app-info {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}

.app-name {
  font-size: 18px;
  font-weight: 600;
  color: #fff;
  margin: 0;
}

.app-version {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.4);
}

.app-origin {
  font-size: 10px;
  color: rgba(255, 255, 255, 0.3);
  display: flex;
  align-items: center;
  gap: 3px;
}

.inline-link {
  border: none;
  background: transparent;
  padding: 0;
  color: rgba(255, 255, 255, 0.45);
  font-size: 10px;
  cursor: pointer;
  text-decoration: underline;
  text-underline-offset: 2px;
}

.inline-link:hover {
  color: var(--focus-color);
}

.fork-changes {
  width: 100%;
  padding: 8px 10px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid rgba(255, 255, 255, 0.07);
}

.changes-title {
  font-size: 10px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.45);
  margin-bottom: 4px;
}

.changes-list {
  margin: 0;
  padding-left: 14px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.changes-list li {
  font-size: 10px;
  line-height: 1.5;
  color: rgba(255, 255, 255, 0.55);
}

.links-row {
  display: flex;
  align-items: center;
  gap: 4px;
}

.link-btn.ghost {
  color: rgba(255, 255, 255, 0.4);
  text-decoration: underline;
  text-underline-offset: 3px;
}

.link-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 16px;
  background: transparent;
  border: none;
  color: rgba(255, 255, 255, 0.6);
  font-size: 13px;
  cursor: pointer;
  transition: color 0.2s;
}

.link-btn:hover {
  color: var(--focus-color);
}

.link-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.link-btn .icon {
  width: 16px;
  height: 16px;
}
</style>
