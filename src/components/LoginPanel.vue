<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import * as api from "../api";

const props = defineProps<{ accountId: string }>();
const sessionOpen = ref(false);
const editing = ref(false);
const online = ref(false);
const busy = ref(false);
const screenshot = ref("");
const secret = ref("");
const message = ref("");
const autoRefresh = ref(false);
const autoConfirm = ref(true);
const mode = ref<"password" | "sms" | "qr">("password");
const pointer = ref({ x: 0.5, y: 0.5 });
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let disposed = false;

const canInteract = computed(() => sessionOpen.value && editing.value && !busy.value);
const modeHelp = computed(() => ({
  password: "先在下方平台画面中点击账号或密码输入框，再发送对应内容。每次只发送到平台当前聚焦的输入框。",
  sms: "请在平台画面中选择短信登录、输入手机号并点击获取验证码，再将收到的验证码发送到已聚焦的验证码框。",
  qr: "请在平台画面中选择扫码登录，使用对应平台的手机应用扫描二维码，并在手机上确认。二维码过期时点击平台原生刷新控件。",
}[mode.value]));

// All browser operations share a gate: background captures never overlap input or finish.
async function run(operation: () => Promise<void>, errorMessage: string) {
  if (busy.value || disposed) return;
  busy.value = true;
  try {
    await operation();
  } catch {
    if (!disposed) message.value = errorMessage;
  } finally {
    busy.value = false;
  }
}

async function captureViewport() {
  const base64 = await api.captureScreenshot(props.accountId);
  if (!disposed) screenshot.value = `data:image/png;base64,${base64}`;
}

async function reconcileSession() {
  if (busy.value || disposed) return;
  await run(async () => {
    const [status, accounts] = await Promise.all([
      api.accountSessionStatus(props.accountId),
      api.listAccounts(),
    ]);
    if (disposed) return;
    sessionOpen.value = status.open;
    editing.value = status.open && status.login_active;
    online.value = status.open && !status.login_active
      && accounts.find((account) => account.id === props.accountId)?.status === "online";
    if (!status.open) {
      secret.value = "";
      screenshot.value = "";
      stopAutoRefresh();
      return;
    }
    ensureRefreshTimer();
    await captureViewport();
  }, "无法读取登录会话状态，请重试。");
}

async function openSession() {
  await run(async () => {
    message.value = "";
    online.value = false;
    await api.openAccountSession(props.accountId);
    sessionOpen.value = true;
    // Opening a browser is not evidence of authentication.
    await api.setLoginActive(props.accountId, true);
    editing.value = true;
    autoConfirm.value = true;
    ensureRefreshTimer();
    message.value = "登录编辑已开启，请在平台画面中完成登录。";
    if (!disposed) await captureViewport();
  }, "无法打开登录编辑，请重试或关闭会话后重新打开。");
}

async function refreshScreenshot() {
  if (!sessionOpen.value) return;
  await run(captureViewport, "画面刷新失败，请重试。");
}

function stopAutoRefresh() {
  autoRefresh.value = false;
  if (refreshTimer !== null) clearTimeout(refreshTimer);
  refreshTimer = null;
}

function ensureRefreshTimer() {
  if (disposed || refreshTimer !== null || !sessionOpen.value) return;
  if (!autoRefresh.value && !(autoConfirm.value && editing.value)) return;
  refreshTimer = setTimeout(async () => {
    refreshTimer = null;
    await run(async () => {
      if (autoConfirm.value && editing.value) await confirmLogin(false);
      if (!disposed && sessionOpen.value && (autoRefresh.value || (autoConfirm.value && editing.value))) {
        await captureViewport();
      }
    }, "自动更新暂时失败，可手动刷新或确认登录。");
    ensureRefreshTimer();
  }, 2500);
}

function toggleAutoRefresh() {
  autoRefresh.value = !autoRefresh.value;
  ensureRefreshTimer();
}

async function sendAction(action: api.LoginAction) {
  if (!canInteract.value) return;
  await run(async () => {
    online.value = false;
    await api.loginAction(props.accountId, action);
    message.value = "操作已发送。可刷新画面确认平台状态。";
    if (!disposed) await captureViewport();
  }, "操作未完成，请确认平台输入框已聚焦后重试。");
}

async function clickViewport(event: MouseEvent) {
  if (!canInteract.value) return;
  const rect = (event.currentTarget as HTMLImageElement).getBoundingClientRect();
  if (!rect.width || !rect.height) return;
  const x = Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width));
  const y = Math.max(0, Math.min(1, (event.clientY - rect.top) / rect.height));
  pointer.value = { x, y };
  await sendAction({ kind: "click", x, y });
}

async function sendText() {
  if (!canInteract.value || !secret.value) return;
  // Clear the reactive/DOM buffer before awaiting IPC. Never expose backend errors
  // or echo the supplied value; the backend types only into the focused field.
  const text = secret.value;
  secret.value = "";
  await sendAction({ kind: "text", text });
}

function sendKey(key: "Tab" | "Backspace" | "Enter") {
  return sendAction({ kind: "key", key });
}

function viewportKey(event: KeyboardEvent) {
  if (event.key === "Enter" || event.key === "Backspace" || event.key === "Tab") {
    event.preventDefault();
    void sendKey(event.key);
  }
}

function scrollViewport(delta: number) {
  return sendAction({ kind: "scroll", ...pointer.value, delta });
}

async function confirmLogin(manual: boolean) {
  const result = await api.finishAccountLogin(props.accountId);
  if (disposed) return;
  if (!result.authenticated || !result.saved) {
    if (manual) {
      message.value = result.authenticated
        ? "平台已登录，但会话尚未保存。请重试完成并保存。"
        : "尚未确认平台登录，请继续在画面中完成登录。";
    }
    // Failed checks must not release the interactive login guard.
    return;
  }
  online.value = true;
  editing.value = false;
  secret.value = "";
  stopAutoRefresh();
  window.dispatchEvent(new CustomEvent("accounts-changed"));
  await api.setLoginActive(props.accountId, false);
  message.value = "登录已确认，会话已安全保存。";
}

async function finishLogin() {
  if (!sessionOpen.value) return;
  secret.value = "";
  await run(() => confirmLogin(true), "未能完成登录确认或退出编辑，请重试；不会自动关闭登录编辑。");
}

async function toggleEditing() {
  secret.value = "";
  await run(async () => {
    const next = !editing.value;
    await api.setLoginActive(props.accountId, next);
    editing.value = next;
    if (next) online.value = false;
    ensureRefreshTimer();
    message.value = next
      ? "登录编辑已开启。请点击平台画面中的目标输入框。"
      : "已结束编辑；这不代表平台已登录或会话已保存。";
  }, "无法切换登录编辑状态，请重试。");
}

async function closeSession() {
  secret.value = "";
  stopAutoRefresh();
  await run(async () => {
    // Closing is an explicit user decision to finish editing.
    await api.setLoginActive(props.accountId, false);
    editing.value = false;
    await api.closeAccountSession(props.accountId);
    sessionOpen.value = false;
    online.value = false;
    screenshot.value = "";
    message.value = "会话已关闭。";
  }, "关闭会话失败，请重试。");
}

function selectMode(next: "password" | "sms" | "qr") {
  mode.value = next;
  secret.value = "";
}

onMounted(() => {
  window.addEventListener("accounts-changed", reconcileSession);
  void reconcileSession();
});

onUnmounted(() => {
  disposed = true;
  window.removeEventListener("accounts-changed", reconcileSession);
  stopAutoRefresh();
  secret.value = "";
  screenshot.value = "";
  // Do not release login-active here: changing tabs is not finishing login.
});
</script>

<template>
  <section class="login-panel" :aria-busy="busy">
    <header class="login-header">
      <div>
        <h3>登录会话</h3>
        <p class="subtitle">平台原生登录 · 安全保存会话</p>
      </div>
      <span class="login-status" :class="{ online }">
        {{ online ? "已确认登录并保存" : sessionOpen ? (editing ? "登录编辑中 · 尚未确认" : "会话已打开 · 编辑已暂停") : "会话未打开" }}
      </span>
    </header>

    <div class="session-actions">
      <button v-if="!sessionOpen" class="primary" :disabled="busy" @click="openSession">
        {{ busy ? "打开中…" : "打开登录会话" }}
      </button>
      <template v-else>
        <button :disabled="busy" @click="refreshScreenshot">刷新画面</button>
        <button :aria-pressed="autoRefresh" :disabled="busy" @click="toggleAutoRefresh">
          {{ autoRefresh ? "停止自动刷新" : "自动刷新画面" }}
        </button>
        <button :disabled="busy" @click="toggleEditing">{{ editing ? "结束编辑（不确认登录）" : "继续登录编辑" }}</button>
        <button class="danger" :disabled="busy" @click="closeSession">关闭会话</button>
      </template>
    </div>

    <template v-if="sessionOpen">
      <div class="guide-tabs" role="group" aria-label="登录方式操作说明">
        <button :aria-pressed="mode === 'password'" @click="selectMode('password')">账号密码说明</button>
        <button :aria-pressed="mode === 'sms'" @click="selectMode('sms')">短信验证说明</button>
        <button :aria-pressed="mode === 'qr'" @click="selectMode('qr')">扫码登录说明</button>
      </div>
      <p class="guide">{{ modeHelp }}</p>
      <p class="hint">以上仅切换操作说明，不会切换平台登录方式；请点击画面中的平台原生控件。</p>

      <div class="viewport-frame">
        <img
          v-if="screenshot"
          :src="screenshot"
          alt="平台完整视口截图；点击目标输入框后使用下方控件输入"
          class="viewport-image"
          :class="{ interactive: canInteract }"
          :tabindex="canInteract ? 0 : -1"
          :aria-disabled="!canInteract"
          draggable="false"
          @click="clickViewport"
          @keydown="viewportKey"
        />
        <p v-else class="empty-viewport">暂无画面，请刷新截图。截图仅反映平台当前页面，不会跳转。</p>
      </div>
      <div class="viewport-tools">
        <span class="hint">完整视口 · 点击定位 · 刷新不跳转</span>
        <div class="key-actions">
          <button :disabled="!canInteract" @click="scrollViewport(-450)">向上滚动</button>
          <button :disabled="!canInteract" @click="scrollViewport(450)">向下滚动</button>
          <button :disabled="!canInteract" @click="sendKey('Tab')">Tab 下一项</button>
          <button :disabled="!canInteract" @click="sendKey('Backspace')">退格</button>
          <button :disabled="!canInteract" @click="sendKey('Enter')">Enter 确认</button>
        </div>
      </div>

      <form class="input-form" autocomplete="off" @submit.prevent="sendText">
        <label :for="`login-input-${accountId}`">发送到平台当前聚焦的输入框</label>
        <div class="input-row">
          <input
            :id="`login-input-${accountId}`"
            v-model="secret"
            type="password"
            autocomplete="off"
            autocapitalize="off"
            :spellcheck="false"
            :disabled="!canInteract"
            placeholder="账号、密码或验证码（内容隐藏）"
          />
          <button type="submit" :disabled="!canInteract || !secret">发送并清空</button>
        </div>
        <p class="hint">先点击画面内的目标输入框。此处不会自动选择字段；发送后立即清空，不记录输入内容。</p>
      </form>
      <label class="auto-confirm">
        <input v-model="autoConfirm" type="checkbox" :disabled="busy" @change="ensureRefreshTimer" />
        自动确认并保存登录（编辑期间每 2.5 秒检查当前页面，不会跳转）
      </label>
      <footer class="finish-row">
        <p class="hint">在平台完成登录后确认。仅打开会话不会标记在线。</p>
        <button class="primary" :disabled="busy" @click="finishLogin">确认登录并保存会话</button>
      </footer>
    </template>
    <p v-if="message" class="login-message" role="status" aria-live="polite">{{ message }}</p>
  </section>
</template>

<style scoped>
.login-panel { background: var(--bg-secondary); border: 1px solid var(--border); border-radius: var(--radius); padding: 18px 20px; }
.login-header, .viewport-tools, .finish-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
h3 { font-size: 13px; font-weight: 600; color: var(--text-secondary); letter-spacing: .5px; }
.subtitle { margin-top: 5px; font-size: 12px; color: var(--text-muted); }
.login-status { border: 1px solid var(--border); border-radius: 99px; padding: 5px 10px; font-size: 12px; color: var(--text-secondary); }
.login-status.online { color: var(--success); border-color: var(--success); }
.session-actions, .key-actions { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
.session-actions { margin-top: 16px; }
.guide-tabs { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 20px; padding-bottom: 10px; border-bottom: 1px solid var(--border); }
.guide-tabs button { border-color: transparent; font-size: 12px; }
.guide-tabs button[aria-pressed="true"] { border-color: var(--border); color: var(--text-primary); background: var(--bg-tertiary, var(--bg-secondary)); }
.guide { margin-top: 12px; font-size: 13px; line-height: 1.7; color: var(--text-secondary); }
.hint { color: var(--text-muted); font-size: 12px; line-height: 1.7; }
.viewport-frame { margin-top: 14px; border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; background: #fff; }
.viewport-image { display: block; width: 100%; height: auto; cursor: default; }
.viewport-image.interactive { cursor: crosshair; }
.viewport-image:focus-visible { outline: 3px solid var(--success); outline-offset: -3px; }
.empty-viewport { padding: 40px 16px; text-align: center; color: #575b62; font-size: 12px; }
.viewport-tools { margin-top: 10px; }
.key-actions button { font-size: 12px; }
.input-form { margin-top: 18px; padding-top: 16px; border-top: 1px solid var(--border); }
.input-form label { display: block; margin-bottom: 7px; color: var(--text-secondary); font-size: 12px; }
.input-row { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 6px; }
.input-row input { flex: 1; min-width: 180px; }
.auto-confirm { display: flex; align-items: center; gap: 8px; margin-top: 16px; color: var(--text-secondary); font-size: 12px; line-height: 1.7; }
.auto-confirm input { flex: none; width: auto; }
.finish-row { margin-top: 18px; padding-top: 14px; border-top: 1px solid var(--border); }
.login-message { margin-top: 14px; padding: 10px 12px; border-left: 2px solid var(--border); color: var(--text-secondary); font-size: 12px; line-height: 1.7; }
</style>
