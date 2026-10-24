<script setup lang="ts">
defineEmits<{ click: [] }>()
</script>

<template>
  <button class="page-back-btn" type="button" aria-label="返回" @click="$emit('click')">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M15 18l-6-6 6-6" />
    </svg>
  </button>
</template>

<style scoped>
.page-back-btn {
  display: grid;
  place-items: center;
  flex-shrink: 0;
  width: 26px;
  height: 26px;
  padding: 0;
  border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.05);
  color: rgba(255, 255, 255, 0.78);
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.page-back-btn:hover {
  background: rgba(255, 255, 255, 0.10);
  color: #fff;
}

.page-back-btn svg {
  width: 14px;
  height: 14px;
}
</style>
