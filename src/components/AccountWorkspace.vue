<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import * as api from "../api";
import LoginPanel from "./LoginPanel.vue";

// accountId 用于各项按账号的命令；platform 用于查平台信息（令牌能力）
const props = defineProps<{ accountId: string; platform: string }>();
const emit = defineEmits<{ close: [id: string] }>();

const tasks = ref<api.PublishTask[]>([]);
const loading = ref(false);

// 发布表单
const title = ref("");
const content = ref("");
const mediaPaths = ref("");
const cronExpr = ref("");
/** 定时发布（本地时间，datetime-local 的值）；与 cron 互斥 */
const scheduledLocal = ref("");const submitting = ref(false);
const message = ref("");

// 敏感词检测
const checking = ref(false);
const sensitiveResult = ref<api.SensitiveCheckResult | null>(null);

// ---------- 平台 API 令牌（仅令牌型平台显示） ----------
const platformInfo = ref<api.PlatformInfo | null>(null);
/** 平台信息是否已读取完成；用于避免在未知能力前闪现浏览器登录面板 */
const platformLoaded = ref(false);
/** 已配置 / 未配置；null 表示状态未知（未读取或不支持令牌） */
const tokenConfigured = ref<boolean | null>(null);
const tokenStateLoading = ref(false);
const tokenInput = ref("");
const tokenSaving = ref(false);
/** 令牌提示文本（沿用本组件既有的 message 风格，但独立于发布表单） */
const tokenMessage = ref("");
const tokenMessageKind = ref<"info" | "ok" | "err">("info");

/** 仅当平台声明支持令牌时显示该区块 */
const tokenSupported = computed(() => platformInfo.value?.tokenSupported === true);
const tokenHint = computed(() => platformInfo.value?.tokenHint ?? null);

const tokenStateText = computed(() => {
  if (!tokenSupported.value) return "";
  if (tokenStateLoading.value) return "读取中…";
  if (tokenConfigured.value === true) return "已配置";
  if (tokenConfigured.value === false) return "未配置";
  return "状态未知";
});

const tokenStateClass = computed(() => {
  if (tokenStateLoading.value) return "unknown";
  if (tokenConfigured.value === true) return "on";
  if (tokenConfigured.value === false) return "off";
  return "unknown";
});

function setTokenMessage(kind: "info" | "ok" | "err", text: string) {
  tokenMessageKind.value = kind;
  tokenMessage.value = text;
}

/** 重新读取「是否已配置令牌」 */
async function refreshTokenConfigured() {
  if (!tokenSupported.value) {
    tokenConfigured.value = null;
    return;
  }
  tokenStateLoading.value = true;
  try {
    tokenConfigured.value = await api.apiTokenConfigured(props.accountId);
  } catch (e) {
    tokenConfigured.value = null;
    setTokenMessage("err", `读取令牌状态失败: ${e}`);
  } finally {
    tokenStateLoading.value = false;
  }
}

/** 读取当前账号所属平台的信息（能力 / 令牌支持 / 令牌提示） */
async function loadPlatformInfo() {
  platformLoaded.value = false;
  try {
    const list = await api.listPlatforms();
    platformInfo.value = list.find((p) => p.id === props.platform) ?? null;
  } catch {
    // 读取失败时按「不支持令牌」处理：显示浏览器登录面板（降级，不锁死用户），
    // 也不显示令牌区块。
    platformInfo.value = null;
  } finally {
    platformLoaded.value = true;
  }
}

async function saveToken() {
  const token = tokenInput.value.trim();
  if (!token) {
    setTokenMessage("err", "请先粘贴 API 令牌");
    return;
  }
  tokenSaving.value = true;
  setTokenMessage("info", "校验并保存中…");
  try {
    await api.savePlatformToken(props.accountId, token);
    tokenInput.value = "";
    // 保存成功后从后端重新读取状态，而不是本地假定成功
    await refreshTokenConfigured();
    setTokenMessage("ok", "令牌已校验并保存");
  } catch (e) {
    // 后端会校验令牌有效性，失败原因对用户很重要，必须原样显示
    setTokenMessage("err", `保存失败: ${e}`);
  } finally {
    tokenSaving.value = false;
  }
}

/** 任务列表的「定时」列：cron 优先，没有则显示一次性定时时间 */
function scheduleText(t: api.PublishTask): string {
  if (t.scheduled_at) {
    return new Date(t.scheduled_at * 1000).toLocaleString();
  }
  return "-";
}

async function loadTasks() {
  loading.value = true;
  try {
    tasks.value = await api.listPublishTasks(props.accountId);
  } catch (e) {
    message.value = `读取任务失败: ${e}`;
  } finally {
    loading.value = false;
  }
}

/** 切换账号 / 平台时重置并重新加载与该账号绑定的数据 */
async function reloadForAccount() {
  title.value = "";
  content.value = "";
  mediaPaths.value = "";
  cronExpr.value = "";
  scheduledLocal.value = "";
  message.value = "";
  sensitiveResult.value = null;
  tokenInput.value = "";
  tokenMessage.value = "";
  tokenConfigured.value = null;
  platformInfo.value = null;
  platformLoaded.value = false;
  manualMessage.value = "";
  await Promise.all([loadTasks(), loadPlatformInfo()]);
  await refreshTokenConfigured();
}

async function checkWords() {
  checking.value = true;
  try {
    sensitiveResult.value = await api.checkSensitiveWords(content.value);
  } catch (e) {
    message.value = `检测失败: ${e}`;
  } finally {
    checking.value = false;
  }
}

async function submitTask() {
  if (!content.value.trim()) {
    message.value = "请输入发布内容";
    return;
  }
  if (cronExpr.value.trim() && scheduledLocal.value) {
    message.value = "定时表达式与定时时间只能选一个";
    return;
  }
  // datetime-local 是本地时间字符串，转成秒级 unix 时间戳（与 DB 列一致）
  let scheduledAt: number | undefined;
  if (scheduledLocal.value) {
    const ms = new Date(scheduledLocal.value).getTime();
    if (Number.isNaN(ms)) {
      message.value = "定时时间格式无效";
      return;
    }
    scheduledAt = Math.floor(ms / 1000);
  }
  submitting.value = true;
  message.value = "";
  try {
    await api.createPublishTask({
      accountId: props.accountId,
      title: title.value || undefined,
      content: content.value,
      mediaPaths: mediaPaths.value || undefined,
      cronExpr: cronExpr.value || undefined,
      scheduledAt,
    });
    message.value = scheduledAt ? "任务已创建，到点自动发布" : "任务已创建";
    title.value = "";
    content.value = "";
    mediaPaths.value = "";
    cronExpr.value = "";
    scheduledLocal.value = "";
    await loadTasks();
  } catch (e) {
    message.value = `创建失败: ${e}`;
  } finally {
    submitting.value = false;
  }
}

// 手动发布窗口：可见 Chrome，独立 profile，登录态与工作台共用一份加密 cookie
const manualBusy = ref(false);
const manualMessage = ref("");
const manualMessageKind = ref<"info" | "ok" | "err">("info");

function setManualMessage(kind: "info" | "ok" | "err", text: string) {
  manualMessageKind.value = kind;
  manualMessage.value = text;
}

async function openManualWindow() {
  manualBusy.value = true;
  setManualMessage("info", "正在打开浏览器窗口…");
  try {
    const port = await api.openManualWindow(props.accountId);
    setManualMessage("ok", `窗口已打开（调试端口 ${port}）。在窗口里完成登录或发布后，记得点「保存登录态」。`);
  } catch (e) {
    setManualMessage("err", `打开窗口失败: ${e}`);
  } finally {
    manualBusy.value = false;
  }
}

async function saveManualCookies() {
  manualBusy.value = true;
  setManualMessage("info", "正在读取窗口登录态…");
  try {
    const count = await api.saveManualWindowCookies(props.accountId);
    setManualMessage("ok", `已保存 ${count} 个 cookie，工作台会话下次打开会直接复用。`);
  } catch (e) {
    setManualMessage("err", `保存失败: ${e}`);
  } finally {
    manualBusy.value = false;
  }
}

async function closeManualWindow() {
  manualBusy.value = true;
  try {
    await api.closeManualWindow(props.accountId);
    setManualMessage("info", "窗口已关闭");
  } catch (e) {
    setManualMessage("err", `关闭失败: ${e}`);
  } finally {
    manualBusy.value = false;
  }
}

// 组件实例会跨账号复用（App.vue 只换 prop），因此切换账号时必须重新加载
watch(() => [props.accountId, props.platform], reloadForAccount);

onMounted(reloadForAccount);
</script>

<template>
  <div class="workspace">
    <div class="workspace-header">
      <h2>发布工作台</h2>
      <div class="header-actions">
        <!-- 浏览器族平台才有可见窗口这一路径；令牌型平台靠 API 发布 -->
        <button
          v-if="platformLoaded && !tokenSupported"
          class="small"
          :disabled="manualBusy"
          title="打开一个可见的 Chrome 窗口，profile 与自动发布实例隔离"
          @click="openManualWindow"
        >
          打开浏览器窗口
        </button>
        <button class="danger" @click="emit('close', accountId)">关闭标签</button>
      </div>
    </div>

    <div class="workspace-body">
      <!-- 令牌型平台没有浏览器登录流程：不渲染登录面板，避免用户进入永远无法成功的路径 -->
      <template v-if="platformLoaded">
        <LoginPanel
          v-if="!tokenSupported"
          :key="accountId"
          :account-id="accountId"
        />
        <section v-else class="panel token-login-guide">
          <h3>使用 API 令牌发布</h3>
          <p class="token-hint">
            该平台通过 API 令牌发布，没有浏览器登录流程。请在下方「平台 API 令牌」中粘贴令牌；
            未配置令牌时无法发布。
          </p>
        </section>
      </template>

      <!-- 手动发布窗口：可见 Chrome，独立 profile，登录态与工作台共用 -->
      <section v-if="platformLoaded && !tokenSupported" class="panel">
        <div class="panel-head">
          <h3>手动发布窗口</h3>
          <span class="manual-actions">
            <button class="small" :disabled="manualBusy" @click="saveManualCookies">保存登录态</button>
            <button class="small" :disabled="manualBusy" @click="closeManualWindow">关闭窗口</button>
          </span>
        </div>
        <p class="token-hint">
          打开一个真实可见的 Chrome 窗口，用独立 profile 与自动发布实例隔离。适合平台未实现自动发布、
          需要自己拖拽上传或富文本编辑的场景。在窗口里登录或切换账号后点「保存登录态」，
          工作台会话下次打开会直接复用。
        </p>
        <p v-if="manualMessage" class="token-message" :class="manualMessageKind">
          {{ manualMessage }}
        </p>
      </section>

      <!-- 平台 API 令牌：仅令牌型平台显示，令牌按账号存储 -->
      <section v-if="tokenSupported" class="panel">
        <div class="panel-head">
          <h3>平台 API 令牌</h3>
          <span class="token-state" :class="tokenStateClass">{{ tokenStateText }}</span>
        </div>
        <p v-if="tokenHint" class="token-hint">令牌获取位置：{{ tokenHint }}</p>
        <p v-else class="token-hint">该平台通过 API 令牌发布。</p>
        <div class="token-form">
          <input
            v-model="tokenInput"
            type="password"
            autocomplete="off"
            placeholder="粘贴 API 令牌"
            @keyup.enter="saveToken"
          />
          <button class="primary" :disabled="tokenSaving || !tokenInput.trim()" @click="saveToken">
            {{ tokenSaving ? "校验中…" : "保存令牌" }}
          </button>
        </div>
        <p v-if="tokenMessage" class="token-message" :class="tokenMessageKind">
          {{ tokenMessage }}
        </p>
      </section>

      <!-- 新建发布任务 -->
      <section class="panel">
        <h3>新建发布任务</h3>
        <div class="form">
          <label>
            标题
            <input v-model="title" placeholder="可选" />
          </label>
          <label>
            内容
            <textarea v-model="content" rows="4" placeholder="发布正文..."></textarea>
          </label>
          <div class="sensitive-check">
            <button
              class="small"
              :disabled="checking || !content.trim()"
              @click="checkWords"
            >
              {{ checking ? "检测中..." : "检测敏感词" }}
            </button>
            <span v-if="sensitiveResult" class="sensitive-result" :class="sensitiveResult.count > 0 ? 'warn' : 'ok'">
              <template v-if="sensitiveResult.count > 0">
                命中 {{ sensitiveResult.count }} 个：{{ sensitiveResult.hits.join("、") }}
              </template>
              <template v-else>未命中敏感词</template>
            </span>
          </div>
          <label>
            媒体文件路径（逗号分隔）
            <input v-model="mediaPaths" placeholder="/path/to/video.mp4, /path/to/image.jpg" />
          </label>
          <label>
            定时表达式（cron，留空则立即发布）
            <input v-model="cronExpr" placeholder="0 9 * * *" :disabled="!!scheduledLocal" />
          </label>
          <label>
            定时时间（本地时间，只发布一次；与 cron 二选一）
            <input
              v-model="scheduledLocal"
              type="datetime-local"
              :disabled="!!cronExpr.trim()"
            />
          </label>
          <div class="form-actions">
            <button class="primary" :disabled="submitting" @click="submitTask">
              {{ submitting ? "提交中..." : "创建任务" }}
            </button>
            <span v-if="message" class="message">{{ message }}</span>
          </div>
        </div>
      </section>

      <!-- 任务列表 -->
      <section class="panel">
        <div class="panel-head">
          <h3>任务列表</h3>
          <button class="small" :disabled="loading" @click="loadTasks">
            {{ loading ? "刷新中..." : "刷新" }}
          </button>
        </div>
        <div v-if="loading" class="empty">加载中...</div>
        <div v-else-if="tasks.length === 0" class="empty">暂无任务</div>
        <table v-else class="task-table">
          <thead>
            <tr>
              <th>标题</th>
              <th>状态</th>
              <th>定时</th>
              <th>创建时间</th>
              <th>结果</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="t in tasks" :key="t.id">
              <td>{{ t.title || t.content?.slice(0, 20) || "-" }}</td>
              <td><span class="task-status" :class="t.status">{{ t.status }}</span></td>
              <td>{{ t.cron_expr || scheduleText(t) }}</td>
              <td>{{ new Date(t.created_at * 1000).toLocaleString() }}</td>
              <td class="result">{{ t.result || "-" }}</td>
            </tr>
          </tbody>
        </table>
      </section>
    </div>
  </div>
</template>

<style scoped>
.panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.workspace {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}

.workspace-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 20px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-secondary);
}

.workspace-header h2 {
  font-size: 15px;
  font-weight: 600;
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.manual-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.workspace-body {
  flex: 1;
  overflow-y: auto;
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.panel {
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 18px 20px;
}

.panel h3 {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 14px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* ---------- 平台 API 令牌 ---------- */
.token-state {
  flex-shrink: 0;
  padding: 2px 9px;
  font-size: 11.5px;
  border-radius: 99px;
}

.token-state.on {
  color: var(--success);
  background: rgba(52, 201, 142, 0.12);
}

.token-state.off {
  color: var(--warning);
  background: rgba(245, 166, 35, 0.12);
}

.token-state.unknown {
  color: var(--text-muted);
  background: var(--bg-tertiary);
}

.token-hint {
  margin-bottom: 10px;
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.5;
}

.token-form {
  display: flex;
  gap: 8px;
}

.token-form input {
  flex: 1;
  min-width: 0;
}

.token-message {
  margin-top: 8px;
  font-size: 12px;
  line-height: 1.5;
  word-break: break-word;
}

.token-message.ok {
  color: var(--success);
}

.token-message.err {
  color: var(--danger);
}

.token-message.info {
  color: var(--text-secondary);
}

.form label {
  display: flex;
  flex-direction: column;
  gap: 5px;
  font-size: 12px;
  color: var(--text-secondary);
}

.form-actions {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 4px;
}

.sensitive-check {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: -4px;
}

.sensitive-result {
  font-size: 12px;
}

.sensitive-result.warn {
  color: var(--warning);
}

.sensitive-result.ok {
  color: var(--success);
}

.message {
  font-size: 12px;
  color: var(--text-secondary);
  margin-top: 10px;
}

.task-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

.task-table th {
  text-align: left;
  padding: 8px 10px;
  color: var(--text-muted);
  font-weight: 500;
  font-size: 12px;
  border-bottom: 1px solid var(--border);
}

.task-table td {
  padding: 9px 10px;
  border-bottom: 1px solid var(--border);
}

.task-status {
  padding: 2px 8px;
  border-radius: 99px;
  font-size: 12px;
}

.task-status.pending {
  color: var(--warning);
  background: rgba(245, 166, 35, 0.12);
}

.task-status.done {
  color: var(--success);
  background: rgba(52, 201, 142, 0.12);
}

.task-status.failed {
  color: var(--danger);
  background: rgba(240, 82, 79, 0.12);
}

.result {
  color: var(--text-muted);
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.empty {
  color: var(--text-muted);
  text-align: center;
  padding: 24px 0;
  font-size: 13px;
}
</style>
