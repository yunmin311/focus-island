<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import { emailInboxUrl } from "../utils/emailLinks";
import { useIslandState } from "../composables/useIslandState";
import { useTimer, type NotificationType } from "../composables/useTimer";
import { useSettings, shouldMirrorSystemNotification, type Settings } from "../composables/useSettings";
import { useDockMode } from "../composables/useDockMode";
import CapsuleIdle from "./CapsuleIdle.vue";
import DockStrip from "./DockStrip.vue";
import CapsuleFocus from "./CapsuleFocus.vue";
import NotificationBanner from "./NotificationBanner.vue";
import SystemPeekBanner from "./SystemPeekBanner.vue";
import {
    emailNotificationRecordId, systemNotificationRecordId, useExternalNotificationSilence,
    type SystemNotificationPayload, type EmailNotificationPayload,
} from "../composables/useNotifications";
import type { TimerStatePayload } from "../composables/useTimerBridge";

const { state, setState, setNotificationVisible } = useIslandState();
const timer = useTimer();
const externalNotificationsSilent = useExternalNotificationSilence();
const { settings, applyExternalSettings } = useSettings();
const { dockCollapsed, stripProgress, stripColor, DOCK_STRIP_WINDOW_H } = useDockMode();
const showStrip = computed(() => dockCollapsed.value);

let unlisten: (() => void) | null = null;
let unlistenSettings: (() => void) | null = null;
let unlistenPanelMotion: (() => void) | null = null;
let unlistenRadio: (() => void) | null = null;
let unlistenNotification: (() => void) | null = null;
let unlistenSystemNotification: (() => void) | null = null;
let unlistenTaskReminder: (() => void) | null = null;
let unlistenEmail: (() => void) | null = null;
let activityTimer: ReturnType<typeof setInterval> | null = null;
let panelStateTimer: ReturnType<typeof setInterval> | null = null;

const radioPlaying = ref(false);
const notificationType = ref<NotificationType>(null);

// 系统通知镜像的岛内 peek：无按钮、自动消失，点击直接打开对应来源
interface SystemPeekItem {
    key: string;
    external: boolean;
    kind: 'system' | 'email' | 'task';
    app: string;
    aumid?: string;
    provider?: string;
    taskId?: string;
    title: string;
    body: string;
    receivedAt: number;
}

interface TaskReminderPayload {
    id: string;
    title: string;
    time: string;
    recordId: string;
}
const systemPeek = ref<SystemPeekItem | null>(null);
const systemPeekQueue: SystemPeekItem[] = [];
let systemPeekTimer: number | null = null;
let systemPeekStartedAt = 0;
let systemPeekRemaining = 0;
const SYSTEM_PEEK_MS = 5000;
const SYSTEM_PEEK_QUEUE_LIMIT = 2;
const SYSTEM_PEEK_MAX_AGE_MS = 15000;

const shellRef = ref<HTMLElement | null>(null);
const peekRef = ref<HTMLElement | null>(null);
let layoutObserver: ResizeObserver | null = null;

// 舞台固定大小，形变全部在 DOM 内完成；
// 岛的屏幕矩形上报给 Rust，供面板定位 / 悬停 / 点击判定使用。
async function reportLayout() {
    const el = shellRef.value;
    if (!el) return;
    const r = el.getBoundingClientRect();
    try {
        await invoke("set_island_layout", { x: r.left, y: r.top, w: r.width, h: r.height });
    } catch {
        // window closing
    }
}

async function reportPeekLayout() {
    const el = peekRef.value;
    if (!el) return;
    const r = el.getBoundingClientRect();
    try {
        if (r.width <= 0 || r.height <= 0) {
            await invoke("clear_peek_layout");
            return;
        }
        await invoke("set_peek_layout", { x: r.left, y: r.top, w: r.width, h: r.height });
    } catch {
        // window closing
    }
}

// 通知横幅出现/消失时上报它自己的矩形；没有横幅就清空，避免旧区域继续拦截点击。
// 延迟一次是为了等横幅的 leave 动画结束，避免把正在消失的区域继续当成可点击区。
watch([notificationType, systemPeek, () => settings.value.islandScale], () => {
    nextTick(reportPeekLayout);
    window.setTimeout(reportPeekLayout, 260);
});

// 面板是否打开（用于确保面板可见时舞台始终穿透，不挡住面板顶部按钮）
const panelOpen = ref(false);

// Watch notification visibility and toggle click-through + resize window
watch([notificationType, systemPeek, panelOpen], () => {
    const visible = notificationType.value !== null || systemPeek.value !== null;
    setNotificationVisible(visible);
    // 点击穿透统一由 useDockMode 的横幅悬停检测管理。
});

// 番茄横幅优先级更高：它出现时收起系统 peek，结束后再补上排队的系统通知
watch(notificationType, (type) => {
    if (type) {
        closeSystemPeek();
    } else {
        nextTick(showNextSystemPeek);
    }
});

// 面板打开时收起 peek，避免横幅压在面板上
watch(panelOpen, (open) => {
    if (open) closeSystemPeek(true);
});

// 悬停展开后的胶囊：点击打开专注清单
function onCapsuleClick() {
    if (showStrip.value) return;
    void invoke("toggle_panel");
}

// 形变 / 缩放 / 通知后上报新矩形
watch([showStrip, notificationType, systemPeek, () => settings.value.islandScale], () => {
    nextTick(reportLayout);
});

function playNotificationSound() {
    if (!settings.value.notificationSound) return;
    try {
        const ctx = new AudioContext();
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.connect(gain);
        gain.connect(ctx.destination);
        osc.type = "sine";
        osc.frequency.setValueAtTime(880, ctx.currentTime);
        osc.frequency.setValueAtTime(1100, ctx.currentTime + 0.08);
        osc.frequency.setValueAtTime(880, ctx.currentTime + 0.16);
        gain.gain.setValueAtTime(0.3, ctx.currentTime);
        gain.gain.exponentialRampToValueAtTime(0.01, ctx.currentTime + 0.35);
        osc.start(ctx.currentTime);
        osc.stop(ctx.currentTime + 0.35);
    } catch {
        // Audio not available
    }
}

function clearSystemPeekTimer() {
    if (systemPeekTimer !== null) {
        window.clearTimeout(systemPeekTimer);
        systemPeekTimer = null;
    }
}

function closeSystemPeek(clearQueue = false) {
    clearSystemPeekTimer();
    systemPeek.value = null;
    if (clearQueue) systemPeekQueue.length = 0;
}

function scheduleSystemPeekEnd(ms: number) {
    clearSystemPeekTimer();
    systemPeekStartedAt = Date.now();
    systemPeekRemaining = ms;
    systemPeekTimer = window.setTimeout(() => {
        systemPeekTimer = null;
        systemPeek.value = null;
        showNextSystemPeek();
    }, ms);
}

function showNextSystemPeek() {
    if (systemPeek.value || notificationType.value) return;
    while (systemPeekQueue.length > 0) {
        const next = systemPeekQueue.shift()!;
        if (Date.now() - next.receivedAt > SYSTEM_PEEK_MAX_AGE_MS) continue;
        if (next.external && externalNotificationsSilent.value) continue;
        systemPeek.value = next;
        scheduleSystemPeekEnd(SYSTEM_PEEK_MS);
        return;
    }
}

// 悬停暂停自动收起：鼠标移入时冻结剩余时间，移出后继续
function onSystemPeekHold() {
    if (systemPeekTimer === null) return;
    const elapsed = Date.now() - systemPeekStartedAt;
    systemPeekRemaining = Math.max(600, systemPeekRemaining - elapsed);
    clearSystemPeekTimer();
}

function onSystemPeekResume() {
    if (!systemPeek.value || systemPeekTimer !== null) return;
    scheduleSystemPeekEnd(systemPeekRemaining > 0 ? systemPeekRemaining : SYSTEM_PEEK_MS);
}

function pushPeekItem(item: SystemPeekItem) {
    // 面板已打开时不再弹 peek：横幅会盖住面板顶部
    if (panelOpen.value) return;
    if (systemPeekQueue.length >= SYSTEM_PEEK_QUEUE_LIMIT) {
        systemPeekQueue.shift();
    }
    systemPeekQueue.push(item);
    showNextSystemPeek();
}

watch(externalNotificationsSilent, (silent) => {
    if (!silent) return;
    for (let index = systemPeekQueue.length - 1; index >= 0; index--) {
        if (systemPeekQueue[index].external) systemPeekQueue.splice(index, 1);
    }
    if (systemPeek.value?.external) {
        closeSystemPeek();
        showNextSystemPeek();
    }
});

function enqueueSystemPeek(payload: SystemNotificationPayload) {
    // 设置页的镜像总开关 / 来源屏蔽
    if (!shouldMirrorSystemNotification(payload.app, payload.aumid)) return;
    if (externalNotificationsSilent.value) return;
    const app = (payload.app ?? "").trim();
    const title = (payload.title ?? "").trim();
    const body = (payload.body ?? "").trim();
    if (!app && !title && !body) return;
    pushPeekItem({
        key: systemNotificationRecordId(payload),
        external: true,
        kind: 'system',
        app,
        aumid: (payload.aumid ?? '').trim() || undefined,
        title,
        body,
        receivedAt: Date.now(),
    });
}

function enqueueTaskReminder(payload: TaskReminderPayload) {
    const title = (payload.title ?? "").trim();
    if (!title) return;
    playNotificationSound();
    pushPeekItem({
        key: payload.recordId,
        external: false,
        kind: 'task',
        app: "每日任务",
        taskId: payload.id,
        title,
        body: `${payload.time} 到点了`,
        receivedAt: Date.now(),
    });
}

function enqueueEmailPeek(payload: EmailNotificationPayload) {
    if (externalNotificationsSilent.value) return;
    const subject = (payload.subject ?? "").trim() || "(无主题)";
    const from = (payload.from ?? "").trim() || "未知发件人";
    playNotificationSound();
    pushPeekItem({
        key: emailNotificationRecordId(payload),
        external: true,
        kind: 'email',
        app: "邮件",
        provider: payload.provider,
        title: subject,
        body: from,
        receivedAt: Date.now(),
    });
}

async function openNotificationFallback(recordId?: string) {
    try {
        await emitTo("panel", "open-notifications", { recordId, openSource: true });
    } finally {
        await invoke("show_panel", { focus: true });
    }
}

async function onSystemPeekOpen() {
    const item = systemPeek.value;
    if (!item) return;
    const recordId = item.key;
    closeSystemPeek(true);

    // 系统通知：直接激活来源应用，不先打开面板
    if (item.kind === "system" && item.aumid) {
        try {
            await invoke("activate_notification_source", { app: item.app, aumid: item.aumid });
            return;
        } catch {
            // 来源无法激活时回落到通知中心，并保留错误提示
        }
    }

    // 邮件：直接打开对应服务商网页收件箱
    if (item.kind === "email") {
        const url = emailInboxUrl(item.provider);
        if (url) {
            try {
                await open(url);
                return;
            } catch {
                // 浏览器打开失败时回落到记录
            }
        }
    }

    // 任务提醒：直接落到任务详情；仍需面板承载
    if (item.kind === "task" && item.taskId) {
        try {
            await emitTo("panel", "open-task", { taskId: item.taskId });
            await invoke("show_panel", { focus: true });
            return;
        } catch {
            // 任务不存在或面板不可用时回落到通知中心
        }
    }

    await openNotificationFallback(recordId);
}

async function onNotifOpen() {
    // 打开计时区不等于选择休息/专注，保留待确认的计时状态。
    try {
        await emitTo("panel", "open-notifications", { destination: "timer" });
    } finally {
        await invoke("show_panel", { focus: true });
    }
}

function onNotifConfirm() {
    notificationType.value = null;
    emit("notification-action", "confirm");
}

function onNotifExtend() {
    notificationType.value = null;
    emit("notification-action", "extend");
}

function onNotifDismiss() {
    notificationType.value = null;
    emit("notification-action", "dismiss");
}

let panelMotionTimer: number | null = null;
let countdownTimer: number | null = null;
let countdownEndAt = 0;

const BASE_WIDTH = 320;
const BASE_HEIGHT = 35;
const panelMotionState = ref<"idle" | "opening" | "closing">("idle");

const capsuleWidth = computed(() => BASE_WIDTH * (settings.value.islandScale ?? 1));
const capsuleHeight = computed(() => BASE_HEIGHT * (settings.value.islandScale ?? 1));
const capsuleRadius = computed(() => capsuleHeight.value / 2);
// 刘海宽度：窗口与玻璃层必须严格贴合，杜绝外侧光晕
const NOTCH_WIDTH = 160;
const notchWidth = computed(() => NOTCH_WIDTH * (settings.value.islandScale ?? 1));

const progressScale = computed(() => {
    if (state.value === "idle" || state.value === "alert") return 0;
    const v = timer.progress.value;
    if (Number.isNaN(v) || v <= 0) return 0;
    if (v >= 1) return 1;
    return v;
});

const ringColor = computed(() => {
    if (timer.phase.value === "focus" && timer.running.value)
        return "var(--focus-color)";
    if (timer.phase.value === "break" && timer.running.value)
        return "var(--break-color)";
    return "rgba(255,255,255,0.45)";
});

function runPanelMotion(phase: "open" | "close") {
    if (panelMotionTimer) {
        window.clearTimeout(panelMotionTimer);
    }
    panelMotionState.value = phase === "open" ? "opening" : "closing";
    panelMotionTimer = window.setTimeout(() => {
        panelMotionState.value = "idle";
        panelMotionTimer = null;
    }, phase === "open" ? 280 : 160);
}

function stopLocalCountdown() {
    if (countdownTimer !== null) {
        window.clearInterval(countdownTimer);
        countdownTimer = null;
    }
}

function syncRemainingFromEndAt() {
    if (countdownEndAt <= 0) return;
    const msLeft = countdownEndAt - Date.now();
    timer.remaining.value = Math.max(0, Math.ceil(msLeft / 1000));
}

function startLocalCountdown(endAt: number) {
    countdownEndAt = endAt;
    syncRemainingFromEndAt();
    if (countdownTimer !== null) return;
    countdownTimer = window.setInterval(syncRemainingFromEndAt, 200);
}

onMounted(async () => {
    invoke("set_click_through", { ignore: true });

    // 空闲检测喂食：岛窗口是点击穿透的收不到输入，
    // 用 Rust 光标轮询探测系统级活动，派发合成事件重置 useIdle
    activityTimer = setInterval(async () => {
        try {
            if (await invoke<boolean>("cursor_active")) {
                window.dispatchEvent(new MouseEvent("mousemove"));
            }
        } catch {
            // ignore
        }
    }, 1000);

    // 面板可见状态以 Rust 为准兜底同步，避免漏掉 close 事件后永久吞掉 peek。
    panelStateTimer = setInterval(async () => {
        try {
            const visible = await invoke<boolean>("is_panel_visible");
            if (panelOpen.value !== visible) panelOpen.value = visible;
        } catch {
            // ignore
        }
    }, 1500);

    await nextTick();
    reportLayout();
    if (typeof ResizeObserver !== "undefined" && shellRef.value) {
        layoutObserver = new ResizeObserver(() => reportLayout());
        layoutObserver.observe(shellRef.value);
    }

    unlisten = await listen<TimerStatePayload>(
        "timer-state-update",
        ({ payload }) => {
            timer.totalDuration.value = payload.totalDuration;
            timer.running.value = payload.running;
            timer.activeTaskId.value = payload.activeTaskId;
            timer.activeTaskTitle.value = payload.activeTaskTitle ?? null;
            if (payload.phase !== timer.phase.value) {
                timer.phase.value = payload.phase;
            }
            if (payload.running) {
                startLocalCountdown(payload.syncedAt + payload.remaining * 1000);
            } else {
                stopLocalCountdown();
                countdownEndAt = 0;
                timer.remaining.value = payload.remaining;
            }

            if (payload.running && payload.phase === "focus") {
                setState("focus");
            } else if (payload.running && payload.phase === "break") {
                setState("break");
            } else if (
                !payload.running &&
                !payload.activeTaskId &&
                state.value !== "idle" &&
                state.value !== "alert"
            ) {
                setState("idle");
            }
        },
    );

    // 拉取 panel 当前计时快照，避免窗口晚启动时错过状态。
    emit("timer-state-request").catch(() => {});

    unlistenSettings = await listen<Settings>("settings-changed", ({ payload }) => {
        applyExternalSettings(payload);
        nextTick(reportLayout);
    });

    unlistenPanelMotion = await listen<string>("island-panel-motion", ({ payload }) => {
        if (payload === "open" || payload === "close") {
            panelOpen.value = payload === "open";
            runPanelMotion(payload);
        }
    });

    unlistenRadio = await listen<{ playing: boolean; stationId: string | null }>("radio-state-update", ({ payload }) => {
        radioPlaying.value = payload.playing;
    });

    unlistenNotification = await listen<NotificationType>("notification-show", ({ payload }) => {
        notificationType.value = payload;
        if (payload) playNotificationSound();
    });

    // 系统通知镜像：短暂在岛内 peek，无按钮，点击进通知中心
    unlistenSystemNotification = await listen<SystemNotificationPayload>(
        "system-notification",
        ({ payload }) => {
            if (payload) enqueueSystemPeek(payload);
        },
    );

    // 每日任务到点提醒：和系统通知共用同一套 peek 队列
    unlistenTaskReminder = await listen<TaskReminderPayload>(
        "task-reminder",
        ({ payload }) => {
            if (payload) enqueueTaskReminder(payload);
        },
    );

    // 邮件通知：同样走 peek 队列
    unlistenEmail = await listen<EmailNotificationPayload>(
        "email-notification",
        ({ payload }) => {
            if (payload) enqueueEmailPeek(payload);
        },
    );
});

onUnmounted(() => {
    if (activityTimer !== null) clearInterval(activityTimer);
    if (panelStateTimer !== null) clearInterval(panelStateTimer);
    layoutObserver?.disconnect();
    clearSystemPeekTimer();
    systemPeekQueue.length = 0;
    void invoke("clear_peek_layout").catch(() => {});
    unlisten?.();
    unlistenSettings?.();
    unlistenPanelMotion?.();
    unlistenRadio?.();
    unlistenNotification?.();
    unlistenSystemNotification?.();
    unlistenTaskReminder?.();
    unlistenEmail?.();
    stopLocalCountdown();
    if (panelMotionTimer) {
        window.clearTimeout(panelMotionTimer);
    }
});
</script>

<template>
    <div class="island-container w-full h-full flex items-start justify-center">
        <div class="island-stack">
            <div
                class="capsule-motion"
                :class="{
                    'panel-launching': panelMotionState === 'opening',
                    'panel-receiving': panelMotionState === 'closing',
                    'animating': panelMotionState !== 'idle',
                }"
            >
                <div
                    ref="shellRef"
                    class="capsule-shell relative flex items-center justify-center"
                    @click="onCapsuleClick"
                    :class="{
                        'ring-focus':
                            timer.phase.value === 'focus' && timer.running.value,
                        'ring-break':
                            timer.phase.value === 'break' && timer.running.value,
                        'strip-mode': showStrip,
                    }"
                    :style="{
                        '--ring-progress': progressScale,
                        '--ring-color': ringColor,
                        '--ring-opacity': timer.running.value ? 1 : 0.35,
                        '--island-scale': settings.islandScale ?? 1,
                        width: (showStrip ? notchWidth : capsuleWidth) + 'px',
                        height: (showStrip ? DOCK_STRIP_WINDOW_H * (settings.islandScale ?? 1) : capsuleHeight) + 'px',
                        borderRadius: (showStrip ? '0 0 10px 10px' : capsuleRadius + 'px'),
                    }"
                >
                    <Transition name="xfade">
                        <DockStrip
                            v-if="showStrip"
                            :progress="stripProgress"
                            :color="stripColor"
                            :scale="settings.islandScale ?? 1"
                        />
                    <div v-else class="absolute inset-0">
                        <CapsuleIdle v-if="state === 'idle' || state === 'alert'" :radio-playing="radioPlaying" />
                        <CapsuleFocus v-else-if="state === 'focus' || state === 'break'" :radio-playing="radioPlaying" />
                        <svg class="progress-ring" :viewBox="`0 0 ${capsuleWidth} ${capsuleHeight}`" aria-hidden="true">
                            <rect
                                class="ring-progress"
                                :x="1.5 * settings.islandScale"
                                :y="1.5 * settings.islandScale"
                                :width="capsuleWidth - 3 * settings.islandScale"
                                :height="capsuleHeight - 3 * settings.islandScale"
                                :rx="capsuleRadius - 1.5 * settings.islandScale"
                                pathLength="1"
                            />
                        </svg>
                    </div>
                    </Transition>
                </div>
            </div>
            <div ref="peekRef" class="peek-layer">
                <NotificationBanner
                    :type="notificationType"
                    :scale="settings.islandScale ?? 1"
                    @open="onNotifOpen"
                    @confirm="onNotifConfirm"
                    @extend="onNotifExtend"
                    @dismiss="onNotifDismiss"
                />
                <SystemPeekBanner
                    :data="systemPeek"
                    :scale="settings.islandScale ?? 1"
                    @open="onSystemPeekOpen"
                    @hold="onSystemPeekHold"
                    @resume="onSystemPeekResume"
                />
            </div>
        </div>
    </div>
</template>

<style scoped>
.xfade-enter-active,
.xfade-leave-active {
    transition: opacity 0.16s ease;
}

.xfade-enter-from,
.xfade-leave-to {
    opacity: 0;
}

.capsule-shell :deep(.dock-strip) {
    position: absolute;
    inset: 0;
}
.island-container {
    /* items-start justify-center to stick to top edge */
}

.island-stack {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
}

.peek-layer {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
}

.capsule-motion {
    transform-origin: center top;
}

.capsule-motion.animating {
    will-change: transform;
}

.capsule-shell {
    /* V2：主题玻璃壳，材质/环境光随六套主题变化 */
    background: var(--island-shell-bg, var(--v2-shell));
    border: 1px solid var(--v2-line);
    box-shadow:
        inset 0 1px 0 rgba(255, 255, 255, 0.16),
        inset 0 -1px 0 var(--v2-shadow);
    backdrop-filter: blur(16px) saturate(1.2);
    -webkit-backdrop-filter: blur(16px) saturate(1.2);
    transition:
        width 0.32s cubic-bezier(0.16, 1, 0.3, 1),
        height 0.32s cubic-bezier(0.16, 1, 0.3, 1),
        border-radius 0.32s cubic-bezier(0.16, 1, 0.3, 1);
    contain: layout style;
}

.capsule-shell.strip-mode {
    /* 收回刘海：快速动画（比展开更短） */
    transition:
        width 0.15s cubic-bezier(0.3, 0, 0.2, 1),
        height 0.15s cubic-bezier(0.3, 0, 0.2, 1),
        border-radius 0.15s cubic-bezier(0.3, 0, 0.2, 1);
    background: var(--island-shell-bg, var(--v2-shell));
}

/* 浅色模式沿用同一主题壳；不再对图片/像素做反色 */
:root[data-color-mode="light"] .capsule-shell {
    background: var(--island-shell-bg, var(--v2-shell));
    box-shadow:
        inset 0 1px 0 rgba(255, 255, 255, 0.16),
        inset 0 -1px 0 var(--v2-shadow);
}

:root[data-color-mode="light"] .capsule-shell.strip-mode {
    background: var(--island-shell-bg, var(--v2-shell));
}

/* 电波条与进度轨道使用主题语义色 */
.capsule-shell :deep(.wave-bar) {
    background: var(--v2-accent);
}

.capsule-shell :deep(.ring-track) {
    stroke: var(--v2-line);
}

.capsule-shell :deep(.track) {
    background: var(--v2-line);
}

/* 玻璃光效层：顶部光条 + 斜向色散高光（纯装饰，不拦截事件） */
.capsule-glass-fx {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    overflow: hidden;
    pointer-events: none;
}

.capsule-shell.strip-mode .capsule-glass-fx {
    display: none;
}

.capsule-glass-fx::before {
    content: "";
    position: absolute;
    top: 2px;
    left: 50%;
    width: 42%;
    height: 2px;
    border-radius: 999px;
    background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.82), transparent);
    filter: blur(0.4px);
    transform: translateX(-50%);
}

.capsule-glass-fx::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='140' height='140'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.85' numOctaves='2' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='140' height='140' filter='url(%23n)'/%3E%3C/svg%3E");
    mix-blend-mode: overlay;
    opacity: 0.10;
}

.capsule-shell::before,
.capsule-shell::after {
    content: "";
    position: absolute;
    pointer-events: none;
}

.capsule-shell::before {
    inset: 1px;
    border-radius: inherit;
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.08), transparent 56%);
    opacity: 0;
    transform: scale(0.88, 0.82);
}

.capsule-shell::after {
    left: 50%;
    top: 56%;
    width: 42%;
    height: 46%;
    border-radius: 999px;
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.12), rgba(255, 255, 255, 0));
    opacity: 0;
    transform: translate(-50%, -42%) scale(0.42, 0.16);
}

/* ring-focus/ring-break 不再加 box-shadow，避免胶囊四周出现浅色矩形光晕 */

.panel-launching {
    will-change: transform;
    animation: island-release 280ms cubic-bezier(0.2, 0.88, 0.26, 1) both;
}

.panel-launching .capsule-shell::before {
    animation: island-core-open 280ms cubic-bezier(0.2, 0.88, 0.26, 1) both;
}

.panel-launching .capsule-shell::after {
    animation: island-tail-open 280ms cubic-bezier(0.2, 0.88, 0.26, 1) both;
}

.panel-receiving {
    will-change: transform;
    animation: island-receive 160ms cubic-bezier(0.34, 0, 0.72, 0.2) both;
}

.panel-receiving .capsule-shell::before {
    animation: island-core-close 160ms cubic-bezier(0.34, 0, 0.72, 0.2) both;
}

.panel-receiving .capsule-shell::after {
    animation: island-tail-close 160ms cubic-bezier(0.34, 0, 0.72, 0.2) both;
}

@keyframes island-release {
    0% {
        transform: scale(1, 1);
    }

    38% {
        transform: scale(0.96, 0.9);
    }

    100% {
        transform: scale(1, 1);
    }
}

@keyframes island-receive {
    0% {
        transform: scale(1, 1);
    }

    42% {
        transform: scale(0.93, 0.84);
    }

    100% {
        transform: scale(1, 1);
    }
}

@keyframes island-core-open {
    0% {
        opacity: 0;
        transform: scale(0.86, 0.8);
    }

    52% {
        opacity: 1;
        transform: scale(1.02, 0.96);
    }

    100% {
        opacity: 0;
        transform: scale(1.08, 1.02);
    }
}

@keyframes island-core-close {
    0% {
        opacity: 0;
        transform: scale(1.02, 0.96);
    }

    56% {
        opacity: 1;
        transform: scale(0.94, 0.86);
    }

    100% {
        opacity: 0;
        transform: scale(0.82, 0.74);
    }
}

@keyframes island-tail-open {
    0% {
        opacity: 0;
        transform: translate(-50%, -42%) scale(0.42, 0.16);
    }

    46% {
        opacity: 0.78;
        transform: translate(-50%, -16%) scale(0.8, 0.46);
    }

    100% {
        opacity: 0;
        transform: translate(-50%, -2%) scale(0.96, 0.68);
    }
}

@keyframes island-tail-close {
    0% {
        opacity: 0;
        transform: translate(-50%, -2%) scale(0.9, 0.62);
    }

    52% {
        opacity: 0.82;
        transform: translate(-50%, -18%) scale(0.72, 0.34);
    }

    100% {
        opacity: 0;
        transform: translate(-50%, -42%) scale(0.38, 0.14);
    }
}

.progress-ring {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
}

.ring-track,
.ring-progress {
    fill: none;
    stroke-width: 3;
    vector-effect: non-scaling-stroke;
}

.ring-track {
    stroke: var(--v2-line);
}

.ring-progress {
    stroke: var(--ring-color);
    stroke-dasharray: 1;
    stroke-dashoffset: calc(1 - var(--ring-progress));
    stroke-linecap: round;
    opacity: var(--ring-opacity);
    transition:
        stroke-dashoffset 0.4s linear,
        opacity 0.3s ease;
}
</style>
