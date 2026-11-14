<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useTasks } from "../composables/useTasks";
import { useSettings } from "../composables/useSettings";
import { PIXEL_3X5, layoutPixelText } from "../utils/pixelFont";

const { recentFocusTasks } = useTasks();
const { settings } = useSettings()

const props = defineProps<{ radioPlaying?: boolean }>();

// 右侧预留宽度（像素时钟 + 播放波形），单位 px（未乘以 island-scale）
const rightInset = computed(() => (props.radioPlaying ? 144 : 120));

const DEFAULT_MOTTOS = [
    "深呼吸，然后开始",
    "专注当下，一次一件事",
    "每一步都算数",
    "你比想象中更有能力",
    "保持节奏，持续前进",
    "休息也是生产力的一部分",
    "今天的努力，明天的底气",
    "把大象切成小块吃",
    "做完比做好更重要",
    "心流就在下一个番茄钟",
];

const mottos = computed(() => {
    const custom = settings.value.idleMottos;
    return custom && custom.length > 0 ? custom : DEFAULT_MOTTOS;
});

function pickRandomMottos(count: number): string[] {
    const pool = [...mottos.value];
    const result: string[] = [];
    for (let i = 0; i < count && pool.length > 0; i++) {
        const idx = Math.floor(Math.random() * pool.length);
        result.push(pool.splice(idx, 1)[0]);
    }
    return result;
}

interface CarouselItem {
    key: string;
    type: "task" | "motto";
    text: string;
    pomodoroCount?: number;
}

const carouselItems = computed<CarouselItem[]>(() => {
    const tasks = recentFocusTasks.value;
    if (tasks.length === 0) {
        // No tasks at all: only mottos
        return mottos.value.map((m, i) => ({
            key: `motto-${i}`,
            type: "motto" as const,
            text: m,
        }));
    }
    // Tasks + 2 random mottos mixed in
    const items: CarouselItem[] = tasks.map((t) => ({
        key: t.id,
        type: "task" as const,
        text: t.title,
        pomodoroCount: t.pomodoroCount,
    }));
    const randomMottos = pickRandomMottos(2);
    randomMottos.forEach((m, i) => {
        items.push({
            key: `motto-r-${i}-${m}`,
            type: "motto" as const,
            text: m,
        });
    });
    return items;
});

const carouselIndex = ref(0);
let carouselTimer: ReturnType<typeof setInterval> | null = null;

// 空闲态右上角实时时钟：对齐到整分钟刷新，避免最多 15s 的显示滞后
const clock = ref("");
let clockTimer: ReturnType<typeof setTimeout> | null = null;

function updateClock() {
    const d = new Date();
    clock.value = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function scheduleClock() {
    updateClock();
    const d = new Date();
    const delay = (60 - d.getSeconds()) * 1000 - d.getMilliseconds() + 30;
    clockTimer = setTimeout(scheduleClock, delay);
}

// 固定 3px：数字块再小一号，避免像素块本身已经很大时显得笨重
const pixelUnit = 3;
const clockPixels = computed(() => layoutPixelText(clock.value, PIXEL_3X5, pixelUnit, 1));

// Reset index when items change length
watch(
    () => carouselItems.value.length,
    () => {
        carouselIndex.value = 0;
    },
);

const currentItem = computed(() => {
    const items = carouselItems.value;
    if (items.length === 0) return null;
    return items[carouselIndex.value % items.length];
});

onMounted(() => {
    carouselTimer = setInterval(() => {
        const len = carouselItems.value.length;
        if (len > 1) {
            carouselIndex.value = (carouselIndex.value + 1) % len;
        }
    }, 5000);
    scheduleClock();
});

onUnmounted(() => {
    if (carouselTimer) clearInterval(carouselTimer);
    if (clockTimer) clearTimeout(clockTimer);
});
</script>

<template>
    <div class="capsule-idle relative w-full h-full overflow-hidden">
        <TransitionGroup
            tag="div"
            name="carousel"
            class="relative w-full h-full"
        >
            <div
                v-if="currentItem"
                :key="currentItem.key"
                class="absolute inset-0 flex items-center gap-2"
                :class="currentItem.type === 'motto' ? 'justify-center' : ''"
                :style="{
                    paddingLeft: '14px',
                    paddingRight: `${rightInset}px`,
                }"
            >
                <!-- Task item -->
                <template v-if="currentItem.type === 'task'">
                    <span
                        v-if="currentItem.pomodoroCount && currentItem.pomodoroCount > 0"
                        class="pomodoro-pixels shrink-0"
                        :style="{ '--pixel-size': `calc(3px * var(--island-scale, 1))` }"
                    >
                        <i v-for="index in Math.min(currentItem.pomodoroCount, 3)" :key="index" />
                        <span v-if="currentItem.pomodoroCount > 3" class="pomodoro-more">+</span>
                    </span>
                    <span class="truncate flex-1 text-[var(--capsule-text)]" :style="{ fontSize: `calc(13px * var(--island-scale, 1))`, fontWeight: 500 }">{{
                        currentItem.text
                    }}</span>
                </template>
                <!-- Motto item -->
                <template v-else>
                    <span class="truncate text-[var(--capsule-text-dim)]" :style="{ fontSize: `calc(12px * var(--island-scale, 1))` }">{{
                        currentItem.text
                    }}</span>
                </template>
            </div>
        </TransitionGroup>

        <!-- 播放波形 + 时钟：同一组 flex 排列，波形不再压到时间上 -->
        <div class="idle-right">
            <div v-if="props.radioPlaying" class="wave-bars">
                <span class="wave-bar" />
                <span class="wave-bar" />
                <span class="wave-bar" />
            </div>
            <svg
                class="idle-clock"
                :width="clockPixels.width"
                :height="clockPixels.height"
                :viewBox="`0 0 ${clockPixels.width} ${clockPixels.height}`"
                shape-rendering="crispEdges"
                :aria-label="clock"
            >
                <rect
                    v-for="cell in clockPixels.cells"
                    :key="`${cell.x}-${cell.y}`"
                    :x="cell.x"
                    :y="cell.y"
                    :width="pixelUnit"
                    :height="pixelUnit"
                />
            </svg>
        </div>

    </div>
</template>

<style scoped>
.idle-right {
    position: absolute;
    right: 14px;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    gap: calc(6px * var(--island-scale, 1));
    z-index: 2;
}

.pomodoro-pixels {
    display: inline-flex;
    align-items: center;
    gap: calc(3px * var(--island-scale, 1));
    color: var(--idle-color);
    opacity: 0.75;
}

.pomodoro-pixels i {
    display: block;
    width: var(--pixel-size, calc(4px * var(--island-scale, 1)));
    height: var(--pixel-size, calc(4px * var(--island-scale, 1)));
    background: var(--idle-color);
}

.pomodoro-more {
    font-size: calc(9px * var(--island-scale, 1));
    line-height: 1;
}
.idle-clock {
    display: block;
    flex-shrink: 0;
    image-rendering: pixelated;
}

.idle-clock rect {
    fill: var(--focus-color);
}
.carousel-dots {
    position: absolute;
    left: 50%;
    bottom: calc(4px * var(--island-scale, 1));
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: calc(5px * var(--island-scale, 1));
    z-index: 2;
    pointer-events: none;
}

/* 与底边高光呼应的一条极淡轨道，让指示器读起来是玻璃边缘的一部分 */
.carousel-dots::after {
    content: "";
    position: absolute;
    left: calc(-10px * var(--island-scale, 1));
    right: calc(-10px * var(--island-scale, 1));
    top: calc(100% + 1px);
    height: 1px;
    background: linear-gradient(90deg, transparent, var(--capsule-dot-idle), transparent);
    opacity: 0.7;
}

.carousel-dot {
    width: calc(4px * var(--island-scale, 1));
    height: calc(4px * var(--island-scale, 1));
    border-radius: 0;
    background: var(--capsule-dot-idle);
    transition:
        width 0.34s cubic-bezier(0.16, 1, 0.3, 1),
        background-color 0.24s ease;
    animation: dot-in 0.26s ease both;
}

.carousel-dot:nth-child(2) { animation-delay: 0.04s; }
.carousel-dot:nth-child(3) { animation-delay: 0.08s; }
.carousel-dot:nth-child(4) { animation-delay: 0.12s; }
.carousel-dot:nth-child(5) { animation-delay: 0.16s; }
.carousel-dot:nth-child(6) { animation-delay: 0.2s; }

.carousel-dot.is-active {
    width: calc(8px * var(--island-scale, 1));
    background: var(--capsule-dot);
}

@keyframes dot-in {
    from { opacity: 0; transform: translateY(1px); }
    to { opacity: 1; transform: none; }
}

@media (prefers-reduced-motion: reduce) {
    .carousel-dot {
        animation: none;
        transition: background-color 0.24s ease;
    }
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

.carousel-enter-active,
.carousel-leave-active {
    transition: all 0.5s cubic-bezier(0.4, 0, 0.2, 1);
}
.carousel-enter-from {
    opacity: 0;
    transform: translateY(100%);
}
.carousel-leave-to {
    opacity: 0;
    transform: translateY(-100%);
}
</style>
