<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from "vue";
import * as api from "./api";
import AccountSidebar from "./components/AccountSidebar.vue";
import AccountWorkspace from "./components/AccountWorkspace.vue";
import SmsSettings from "./components/SmsSettings.vue";
import CustomPlatforms from "./components/CustomPlatforms.vue";
import McpSettings from "./components/McpSettings.vue";
import BatchPublish from "./components/BatchPublish.vue";

const accounts = ref<api.Account[]>([]);
const openTabs = ref<string[]>([]);
const activeTab = ref<string | null>(null);
const engine = ref<api.EngineStatus>({ instances: [], count: 0 });
const engineLoading = ref(false);
const logs = ref<api.EventLog[]>([]);
const showSettings = ref(false);
const showBatchPublish = ref(false);
const showHelp = ref(false);

const sidebarRef = ref<InstanceType<typeof AccountSidebar> | null>(null);

/** 快捷键说明表 */
const SHORTCUTS: { keys: string; desc: string }[] = [
  { keys: "Ctrl/⌘ + N", desc: "新增账号" },
  { keys: "Ctrl/⌘ + B", desc: "批量发布" },
  { keys: "Ctrl/⌘ + Enter", desc: "批量发布中直接提交" },
  { keys: "Ctrl/⌘ + ,", desc: "打开设置" },
  { keys: "Ctrl/⌘ + Tab", desc: "切换到下一个选项卡" },
  { keys: "Ctrl/⌘ + 1 ~ 9", desc: "跳到第 N 个选项卡" },
  { keys: "Ctrl/⌘ + /", desc: "显示 / 隐藏本帮助" },
  { keys: "Esc", desc: "逐层关闭弹层与右键菜单" },
  { keys: "账号上右键", desc: "检测会话 / 复制 / 删除" },
];

/** 焦点在输入控件内时不劫持按键 */
function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  return (
    el.tagName === "INPUT" ||
    el.tagName === "TEXTAREA" ||
    el.tagName === "SELECT" ||
    el.isContentEditable
  );
}

function onKeydown(e: KeyboardEvent) {
  // Esc：按层级从外到内依次关闭
  if (e.key === "Escape") {
    if (showHelp.value) showHelp.value = false;
    else if (showBatchPublish.value) showBatchPublish.value = false;
    else if (showSettings.value) showSettings.value = false;
    return;
  }

  const mod = e.metaKey || e.ctrlKey;
  if (isTyping(e.target)) return;
  if (!mod) return;

  const key = e.key.toLowerCase();

  if (key === "n") {
    e.preventDefault();
    sidebarRef.value?.openForm();
    return;
  }
  if (key === "b") {
    e.preventDefault();
    showBatchPublish.value = true;
    return;
  }
  if (e.key === ",") {
    e.preventDefault();
    showSettings.value = true;
    return;
  }
  if (e.key === "/" || (e.shiftKey && e.key === "?")) {
    e.preventDefault();
    showHelp.value = !showHelp.value;
    return;
  }
  // Ctrl/⌘ + Tab / Shift + Tab：循环切换选项卡
  if (e.key === "Tab" && openTabs.value.length > 0) {
    e.preventDefault();
    const len = openTabs.value.length;
    const cur = activeTab.value ? openTabs.value.indexOf(activeTab.value) : -1;
    const step = e.shiftKey ? -1 : 1;
    activeTab.value = openTabs.value[(cur + step + len) % len] ?? null;
    return;
  }
  // Ctrl/⌘ + 1~9：跳到第 N 个选项卡
  if (!isTyping(e.target) && /^[1-9]$/.test(e.key)) {
    const idx = Number(e.key) - 1;
    if (idx < openTabs.value.length) {
      e.preventDefault();
      activeTab.value = openTabs.value[idx] ?? null;
    }
  }
}

const engineRunning = computed(() => engine.value.count > 0);
const directInstance = computed(
  () => engine.value.instances.find((i) => i.proxy === "direct") ?? null
);

const activeAccount = computed(() =>
  accounts.value.find((a) => a.id === activeTab.value) ?? null
);

async function loadAccounts() {
  accounts.value = await api.listAccounts();
}

async function loadEngine() {
  try {
    engine.value = await api.engineStatus();
  } catch {
    engine.value = { instances: [], count: 0 };
  }
}

async function loadLogs() {
  try {
    logs.value = await api.recentLogs(20);
  } catch {
    // 忽略
  }
}

async function toggleEngine() {
  engineLoading.value = true;
  try {
    if (engineRunning.value) {
      await api.stopEngine();
    } else {
      await api.startEngine();
    }
    await loadEngine();
  } finally {
    engineLoading.value = false;
  }
}

function openAccount(id: string) {
  if (!openTabs.value.includes(id)) {
    openTabs.value.push(id);
  }
  activeTab.value = id;
}

function closeTab(id: string) {
  openTabs.value = openTabs.value.filter((t) => t !== id);
  if (activeTab.value === id) {
    activeTab.value = openTabs.value[openTabs.value.length - 1] ?? null;
  }
}

function onAccountDeleted(id: string) {
  closeTab(id);
}

onMounted(async () => {
  window.addEventListener("accounts-changed", loadAccounts);
  await Promise.all([loadAccounts(), loadEngine(), loadLogs()]);
  window.addEventListener("keydown", onKeydown);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("accounts-changed", loadAccounts);
});
</script>

<template>
  <div class="app">
    <!-- 顶栏 -->
    <header class="topbar">
      <div class="brand">
        <span class="brand-name">营销通</span>
        <span class="brand-sub">MarketingConnect</span>
      </div>
      <div class="engine-status">
        <span class="badge" :class="engineRunning ? 'online' : 'offline'">
          {{ engineRunning ? `引擎运行中 (${engine.count})` : "引擎未启动" }}
        </span>
        <span v-if="directInstance" class="engine-port">:{{ directInstance.port }}</span>
        <button
          class="batch-btn"
          title="批量发布到多个账号（Ctrl/⌘ + B）"
          @click="showBatchPublish = true"
        >
          批量发布
        </button>
        <button
          class="icon-btn"
          title="快捷键（Ctrl/⌘ + /）"
          aria-label="快捷键"
          @click="showHelp = true"
        >
          <svg
            viewBox="0 0 24 24"
            width="16"
            height="16"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <rect x="2" y="6" width="20" height="12" rx="2" />
            <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M6 14h.01M18 14h.01M9.5 14h5" />
          </svg>
        </button>
        <button
          class="icon-btn"
          title="设置（Ctrl/⌘ + ,）"
          aria-label="设置"
          @click="showSettings = true"
        >
          <svg
            viewBox="0 0 24 24"
            width="16"
            height="16"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <circle cx="12" cy="12" r="3" />
            <path
              d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.6 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
            />
          </svg>
        </button>
        <button :disabled="engineLoading" @click="toggleEngine">
          {{ engineLoading ? "..." : engineRunning ? "停止引擎" : "启动引擎" }}
        </button>
      </div>
    </header>

    <div class="main">
      <!-- 侧边栏：账号管理 -->
      <AccountSidebar
        ref="sidebarRef"
        :accounts="accounts"
        :active-id="activeTab"
        @select="openAccount"
        @refresh="loadAccounts"
        @deleted="onAccountDeleted"
      />

      <!-- 主区域 -->
      <div class="content">
        <!-- 选项卡栏 -->
        <nav v-if="openTabs.length > 0" class="tabs">
          <div
            v-for="id in openTabs"
            :key="id"
            class="tab"
            :class="{ active: id === activeTab }"
            @click="activeTab = id"
          >
            <span>{{ accounts.find((a) => a.id === id)?.name ?? id.slice(0, 8) }}</span>
            <button class="tab-close" @click.stop="closeTab(id)">×</button>
          </div>
        </nav>

        <!-- 工作区 -->
        <div class="workspace-area">
          <AccountWorkspace
            v-if="activeAccount"
            :account-id="activeAccount.id"
            @close="closeTab"
          />
          <div v-else class="placeholder">
            <svg
              class="placeholder-icon"
              viewBox="0 0 24 24"
              width="40"
              height="40"
              fill="none"
              stroke="currentColor"
              stroke-width="1.4"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <rect x="2" y="3" width="20" height="14" rx="2" />
              <path d="M8 21h8M12 17v4" />
            </svg>
            <p>从左侧选择一个账号开始</p>
            <p class="placeholder-hint">
              首次使用请先「启动引擎」，然后新增账号并打开工作台（Ctrl/⌘ + N）
            </p>
          </div>
        </div>

        <!-- 底部日志条 -->
        <footer v-if="logs.length > 0" class="logbar">
          <span v-for="log in logs.slice(0, 3)" :key="log.id" class="log-item" :class="log.level">
            [{{ log.event }}] {{ log.detail || "" }}
          </span>
        </footer>
      </div>
    </div>

    <!-- 批量发布弹层 -->
    <BatchPublish
      v-if="showBatchPublish"
      :accounts="accounts"
      @close="showBatchPublish = false"
    />

    <!-- 设置弹层 -->
    <div v-if="showSettings" class="modal-overlay" @click.self="showSettings = false">
      <div class="modal">
        <div class="modal-header">
          <div>
            <div class="modal-title">设置</div>
            <div class="modal-subtitle">MCP 接入、短信提醒与自定义平台</div>
          </div>
          <button class="modal-close" aria-label="关闭设置" @click="showSettings = false">×</button>
        </div>
        <div class="modal-body">
          <McpSettings />
          <div class="modal-divider" />
          <SmsSettings />
          <div class="modal-divider" />
          <CustomPlatforms />
        </div>
      </div>
    </div>

    <!-- 快捷键帮助 -->
    <div v-if="showHelp" class="modal-overlay" @click.self="showHelp = false">
      <div class="modal help-modal">
        <div class="modal-header">
          <div>
            <div class="modal-title">快捷键</div>
            <div class="modal-subtitle">键盘可达，鼠标右键另有快捷操作</div>
          </div>
          <button class="modal-close" aria-label="关闭快捷键帮助" @click="showHelp = false">×</button>
        </div>
        <div class="modal-body">
          <ul class="shortcut-list">
            <li v-for="s in SHORTCUTS" :key="s.keys">
              <span class="shortcut-desc">{{ s.desc }}</span>
              <kbd>{{ s.keys }}</kbd>
            </li>
          </ul>
        </div>
        <div class="modal-footer">
          <span class="modal-subtitle">随时按 Ctrl/⌘ + / 呼出本表</span>
          <button class="primary" @click="showHelp = false">知道了</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 52px;
  padding: 0 20px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.brand {
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.brand-name {
  font-size: 17px;
  font-weight: 700;
  letter-spacing: 1px;
}

.brand-sub {
  font-size: 11px;
  color: var(--text-muted);
  letter-spacing: 0.5px;
}

.engine-status {
  display: flex;
  align-items: center;
  gap: 10px;
}

.engine-port {
  font-size: 12px;
  color: var(--text-muted);
  font-family: ui-monospace, monospace;
}

.main {
  flex: 1;
  display: flex;
  overflow: hidden;
}

.content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.tabs {
  display: flex;
  gap: 2px;
  padding: 6px 10px 0;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border);
  overflow-x: auto;
  flex-shrink: 0;
}

.tab {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 14px;
  font-size: 13px;
  border-radius: var(--radius) var(--radius) 0 0;
  cursor: pointer;
  color: var(--text-secondary);
  border: 1px solid transparent;
  border-bottom: none;
  white-space: nowrap;
}

.tab:hover {
  background: var(--bg-hover);
}

.tab.active {
  background: var(--bg-primary);
  color: var(--text-primary);
  border-color: var(--border);
}

.tab-close {
  border: none;
  background: none;
  padding: 0 2px;
  font-size: 14px;
  line-height: 1;
  color: var(--text-muted);
}

.tab-close:hover {
  color: var(--danger);
  background: none;
  border: none;
}

.workspace-area {
  flex: 1;
  overflow: hidden;
}

.placeholder {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--text-muted);
}

.placeholder-icon {
  opacity: 0.45;
}

.placeholder p {
  font-size: 14px;
}

.placeholder-hint {
  font-size: 12px !important;
  opacity: 0.7;
}

.logbar {
  display: flex;
  gap: 18px;
  padding: 6px 16px;
  background: var(--bg-secondary);
  border-top: 1px solid var(--border);
  font-size: 11px;
  color: var(--text-muted);
  font-family: ui-monospace, monospace;
  overflow: hidden;
  white-space: nowrap;
  flex-shrink: 0;
}

.log-item.error {
  color: var(--danger);
}

.log-item.warn {
  color: var(--warning);
}

/* 设置弹层 */
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.modal {
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 8px;
  width: 640px;
  max-width: 90vw;
  max-height: 85vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
  font-size: 14px;
  font-weight: 600;
}

.modal-close {
  border: none;
  background: none;
  font-size: 18px;
  color: var(--text-muted);
  cursor: pointer;
  padding: 0 4px;
}

.modal-close:hover {
  color: var(--text-primary);
}

.modal-body {
  overflow-y: auto;
}

.modal-divider {
  height: 1px;
  background: var(--border);
  margin: 4px 0;
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  padding: 0;
  color: var(--text-secondary);
}

.icon-btn:hover:not(:disabled) {
  color: var(--text-primary);
}

.help-modal {
  width: 520px;
}

.help-modal .modal-body {
  padding: 14px 18px;
}

.shortcut-list {
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.shortcut-list li {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 7px 4px;
  border-bottom: 1px solid var(--border);
}

.shortcut-list li:last-child {
  border-bottom: none;
}

.shortcut-desc {
  font-size: 13px;
  color: var(--text-secondary);
}

kbd {
  flex-shrink: 0;
  padding: 2px 8px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 11.5px;
  color: var(--text-primary);
  background: var(--bg-primary);
  border: 1px solid var(--border);
  border-radius: 5px;
}

.batch-btn {
  padding: 5px 12px;
  font-size: 12px;
  font-weight: 500;
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}

.batch-btn:hover:not(:disabled) {
  opacity: 0.9;
  background: var(--accent);
}
</style>
