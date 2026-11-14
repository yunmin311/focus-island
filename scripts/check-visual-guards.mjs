import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const errors = []
const warnings = []

function walk(dir) {
  const out = []
  for (const name of readdirSync(dir)) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) out.push(...walk(path))
    else if (/\.(vue|css|ts)$/.test(name)) out.push(path)
  }
  return out
}

// 结构不变量：面板必须是 420×600、无系统阴影，圆角/阴影只由 CSS 卡片负责
const config = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
const panel = config.app.windows.find((w) => w.label === 'panel')
if (!panel) errors.push('tauri.conf.json: panel window missing')
else {
  if (panel.width !== 420 || panel.height !== 600) errors.push('panel window must stay 420×600')
  if (panel.minWidth !== 320 || panel.minHeight !== 480) errors.push('panel min size must stay 320×480')
  if (panel.shadow !== false) errors.push('panel window shadow must be false')
  if (panel.resizable !== false) errors.push('panel window must not be user-resizable (fixed glass surface)')
  if (panel.skipTaskbar !== true) errors.push('panel window must skip taskbar (no white thumbnail entry)')
}

const files = walk(join(root, 'src'))
for (const file of files) {
  const text = readFileSync(file, 'utf8')
  const rel = relative(root, file)

  // 面板根节点不允许再出现 clip-path；圆角只用 border-radius + overflow
  for (const block of text.matchAll(/\.panel-root\s*\{([^}]*)\}/g)) {
    if (/clip-path/.test(block[1])) errors.push(`${rel}: .panel-root must not use clip-path`)
  }

  // 卡片不允许再挂外阴影（之前“外面泡一层”的来源）
  for (const block of text.matchAll(/\.panel-card\s*\{([^}]*)\}/g)) {
    if (/var\(--panel-shell-shadow|0 18px 50px|0 24px 70px/.test(block[1])) {
      errors.push(`${rel}: .panel-card must not use an outer shadow`)
    }
    if (/overflow:\s*hidden/.test(block[1])) {
      errors.push(`${rel}: .panel-card must use overflow: clip, not hidden (hidden is scrollable → page shifts)`)
    }
  }

  // 断点必须按 320/375/420 面板宽度，不允许留下 373/413/423 的舞台偏移断点
  if (/@media\s*\(max-width:\s*(373|413|423)px\)/.test(text)) {
    errors.push(`${rel}: shifted breakpoint found; use 349/389/399`)
  }

  // 只警告，不阻断：仍有少量旧白色文字/边框需要人工看
  if (/(?<![-\w])color:\s*(#fff|#ffffff|white|rgba\(255,\s*255,\s*255)/i.test(text)) {
    warnings.push(`${rel}: raw white text color remains`)
  }
  if (/border[^;]*rgba\(255,\s*255,\s*255/i.test(text)) {
    warnings.push(`${rel}: raw white border remains`)
  }
}

const settings = readFileSync(join(root, 'src/composables/useSettings.ts'), 'utf8')
if (!/Math\.max\(0\.4,\s*settings\.value\.panelOpacity\)/.test(settings)) {
  errors.push('useSettings.ts: panel opacity clamp must be 0.4–1')
}
if (!/Math\.max\(0\.4,\s*settings\.value\.islandOpacity\)/.test(settings)) {
  errors.push('useSettings.ts: island opacity clamp must be 0.4–1')
}

const timer = readFileSync(join(root, 'src/composables/useTimer.ts'), 'utf8')
if (!/if \(!running\.value\)[\s\S]*remaining\.value = newDuration/.test(timer)) {
  errors.push('useTimer.ts: idle timer must sync to focusDuration/breakDuration')
}

for (const name of ['CapsuleIdle.vue', 'CapsuleFocus.vue']) {
  const text = readFileSync(join(root, 'src/components', name), 'utf8')
  if (!/const pixelUnit = 3/.test(text)) errors.push(`${name}: island pixel unit must be fixed 3px`)
  if (/Math\.round\(3\s*\*/.test(text)) errors.push(`${name}: island pixel unit must not scale with islandScale`)
}

const island = readFileSync(join(root, 'src/components/Island.vue'), 'utf8')
const islandTemplate = island.split('<style')[0]
if (/capsule-glass-fx|ring-track/.test(islandTemplate)) {
  errors.push('Island.vue: island must stay one visual layer (remove glass-fx and ring-track)')
}
if (!/const BASE_HEIGHT = 35/.test(island)) {
  errors.push('Island.vue: island height must stay 35 (one size down from 38)')
}

console.log('Visual guards:')
if (errors.length === 0) console.log('  ✓ structural invariants OK')
for (const error of errors) console.log(`  ✗ ${error}`)
for (const warning of warnings) console.log(`  ! ${warning}`)

console.log('\nManual visual checklist (run after every visual change):')
console.log('  [ ] 320 / 375 / 420：无横向溢出，底栏 320/375 竖排、420 横排')
console.log('  [ ] Liquid 浅/深 + Ocean 浅/深：文字对比度、强调按钮 on-accent')
console.log('  [ ] 面板 40% / 70% / 100%：单层磨砂玻璃，文字仍可读')
console.log('  [ ] 面板圆角外没有系统阴影/光晕/双层边')
console.log('  [ ] 首页番茄钟时长与设置一致（30 分钟 → 30:00）')
console.log('  [ ] 添加任务是搜索框样式，输入区与右侧按钮不重叠')
console.log('  [ ] 灵动岛 3px 数字清晰，文字不被裁切')
console.log('  [ ] 设置里“显示模式/显示方式”是整行分段控件，无竖线/双层边')
console.log('  [ ] TODO / 备忘 / 统计 / 电台 / 通知 / 设置逐页过一遍')

if (errors.length > 0) process.exit(1)
