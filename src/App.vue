<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import * as api from "./api";
import AccountSidebar from "./components/AccountSidebar.vue";
import AccountWorkspace from "./components/AccountWorkspace.vue";
import SmsSettings from "./components/SmsSettings.vue";
import CustomPlatforms from "./components/CustomPlatforms.vue";
import BatchPublish from "./components/BatchPublish.vue";

const accounts = ref<api.Account[]>([]);
const openTabs = ref<string[]>([]);
const activeTab = ref<string | null>(null);
const engine = ref<api.EngineStatus>({ instances: [], count: 0 });
const engineLoading = ref(false);
const logs = ref<api.EventLog[]>([]);
const showSettings = ref(false);
const showBatchPublish = ref(false);

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
  await Promise.all([loadAccounts(), loadEngine(), loadLogs()]);
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
          title="批量发布到多个账号"
          @click="showBatchPublish = true"
        >
          批量发布
        </button>
        <button class="settings-btn" title="短信提醒设置" @click="showSettings = true">
          ⚙
        </button>
        <button :disabled="engineLoading" @click="toggleEngine">
          {{ engineLoading ? "..." : engineRunning ? "停止引擎" : "启动引擎" }}
        </button>
      </div>
    </header>

    <div class="main">
      <!-- 侧边栏：账号管理 -->
      <AccountSidebar
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
            <div class="placeholder-icon">🖥️</div>
            <p>从左侧选择一个账号开始</p>
            <p class="placeholder-hint">
              首次使用请先「启动引擎」，然后新增账号并打开工作台
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
          <span>设置</span>
          <button class="modal-close" @click="showSettings = false">×</button>
        </div>
        <div class="modal-body">
          <SmsSettings />
          <div class="modal-divider" />
          <CustomPlatforms />
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
  font-size: 40px;
  opacity: 0.5;
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

.settings-btn {
  padding: 5px 9px;
  font-size: 14px;
  line-height: 1;
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
