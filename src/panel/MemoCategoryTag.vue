<script setup lang="ts">
import { computed } from 'vue'
import { useMemos, getCategoryColor } from '../composables/useMemos'

const props = defineProps<{
  categoryId: string
}>()

const { categories } = useMemos()

const category = computed(() => {
  return categories.value.find(c => c.id === props.categoryId)
})

const categoryName = computed(() => {
  return category.value?.name || '未分类'
})

const colorStyle = computed(() => {
  const colorId = category.value?.color || 'yellow'
  const color = getCategoryColor(colorId)
  return {
    backgroundColor: color.bg,
    borderColor: color.border,
    // 文字用主题 ink 保证可读，颜色只留给图标/背景做分类标识
    '--tag-accent': color.icon
  }
})
</script>

<template>
  <span class="memo-category-tag" :style="colorStyle">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
    </svg>
    {{ categoryName }}
  </span>
</template>

<style scoped>
@reference "../styles.css";

.memo-category-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  padding: 3px 8px;
  border-radius: 4px;
  background: var(--ui-well);
  border: 1px solid var(--v2-line);
  color: var(--v2-ink);
  font-weight: 500;
}

.memo-category-tag svg {
  width: 12px;
  height: 12px;
  color: var(--tag-accent, var(--v2-accent));
}
</style>
