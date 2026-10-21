<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import * as api from "../api";

const props = defineProps<{ accountId: string }>();
const emit = defineEmits<{ close: [id: string] }>();

const tasks = ref<api.PublishTask[]>([]);
const loading = ref(false);

// 会话状态
const sessionOpen = ref(false);
const sessionChecking = ref(false);
const sessionMessage = ref("");

// 页面截图（登录辅助）
const screenshot = ref("");
const capturing = ref(false);
const autoRefresh = ref(false);
let refreshTimer: ReturnType<typeof setInterval> | null = null;

// 发布表单
const title = ref("");
const content = ref("");
const mediaPaths = ref("");
const cronExpr = ref("");
const submitting = ref(false);
const message = ref("");

async function loadTasks() {
  loading.value = true;
  try {
    tasks.value = await api.listPublishTasks(props.accountId);
  } finally {
    loading.value = false;
  }
}

async function openSession() {
  sessionMessage.value = "";
  try {
    await api.openAccountSession(props.accountId);
    sessionOpen.value = true;
    sessionMessage.value = "会话已打开";
    await capture();
  } catch (e) {
    sessionMessage.value = `打开失败: ${e}`;
  }
}

async function closeSession() {
  stopAutoRefresh();
  try {
    await api.closeAccountSession(props.accountId);
    sessionOpen.value = false;
    screenshot.value = "";
    sessionMessage.value = "会话已关闭";
  } catch (e) {
    sessionMessage.value = `关闭失败: ${e}`;
  }
}

async function checkSession() {
  sessionChecking.value = true;
  sessionMessage.value = "";
  try {
    const valid = await api.checkAccountSession(props.accountId);
    sessionOpen.value = true;
    sessionMessage.value = valid ? "会话有效" : "会话已掉线，请重新登录";
    await capture();
  } catch (e) {
    sessionMessage.value = `检测失败: ${e}`;
  } finally {
    sessionChecking.value = false;
  }
}

async function saveCookies() {
  sessionMessage.value = "";
  try {
    const count = await api.saveAccountCookies(props.accountId);
    sessionMessage.value = `已保存 ${count} 个 cookie（加密存储）`;
  } catch (e) {
    sessionMessage.value = `保存失败: ${e}`;
  }
}

async function capture() {
  capturing.value = true;
  try {
    const base64 = await api.captureScreenshot(props.accountId);
    screenshot.value = `data:image/png;base64,${base64}`;
  } catch (e) {
    sessionMessage.value = `截图失败: ${e}`;
  } finally {
    capturing.value = false;
  }
}

function startAutoRefresh() {
  autoRefresh.value = true;
  capture();
  refreshTimer = setInterval(capture, 3000);
}

function stopAutoRefresh() {
  autoRefresh.value = false;
  if (refreshTimer) {
    clearInterval(refreshTimer);
    refreshTimer = null;
  }
}

async function submitTask() {
  if (!content.value.trim()) {
    message.value = "请输入发布内容";
    return;
  }
  submitting.value = true;
  message.value = "";
  try {
    await api.createPublishTask({
      account_id: props.accountId,
      title: title.value || undefined,
      content: content.value,
      media_paths: mediaPaths.value || undefined,
      cron_expr: cronExpr.value || undefined,
    });
    message.value = "任务已创建";
    title.value = "";
    content.value = "";
    mediaPaths.value = "";
    cronExpr.value = "";
    await loadTasks();
  } catch (e) {
    message.value = `创建失败: ${e}`;
  } finally {
    submitting.value = false;
  }
}

onMounted(loadTasks);
onUnmounted(stopAutoRefresh);
</script>

<template>
  <div class="workspace">
    <div class="workspace-header">
      <h2>发布工作台</h2>
      <button class="danger" @click="emit('close', accountId)">关闭标签</button>
    </div>

    <div class="workspace-body">
      <!-- 会话管理 -->
      <section class="panel">
        <h3>登录会话</h3>
        <div class="session-bar">
          <span class="badge" :class="sessionOpen ? 'online' : 'offline'">
            {{ sessionOpen ? "会话已打开" : "会话未打开" }}
          </span>
          <div class="session-actions">
            <button v-if="!sessionOpen" class="primary" @click="openSession">打开会话</button>
            <template v-else>
              <button :disabled="sessionChecking" @click="checkSession">
                {{ sessionChecking ? "检测中..." : "检测会话" }}
              </button>
              <button :disabled="capturing" @click="capture">
                {{ capturing ? "截图中..." : "刷新截图" }}
              </button>
              <button
                :class="autoRefresh ? 'primary' : ''"
                @click="autoRefresh ? stopAutoRefresh() : startAutoRefresh()"
              >
                {{ autoRefresh ? "停止自动刷新" : "自动刷新" }}
              </button>
              <button @click="saveCookies">保存 Cookie</button>
              <button class="danger" @click="closeSession">关闭会话</button>
            </template>
          </div>
        </div>
        <p v-if="sessionMessage" class="message">{{ sessionMessage }}</p>

        <!-- 页面截图（无头模式登录辅助） -->
        <div v-if="sessionOpen" class="screenshot-area">
          <div v-if="screenshot" class="screenshot-frame">
            <img :src="screenshot" alt="页面截图" />
          </div>
          <p v-else class="screenshot-hint">
            截图加载中... 若长时间无画面，请点击「刷新截图」
          </p>
          <p class="screenshot-tip">
            提示：在下方画面中完成登录（扫码或输入帐密），登录成功后点击「保存 Cookie」持久化会话
          </p>
        </div>
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
          <label>
            媒体文件路径（逗号分隔）
            <input v-model="mediaPaths" placeholder="/path/to/video.mp4, /path/to/image.jpg" />
          </label>
          <label>
            定时表达式（cron，留空则立即发布）
            <input v-model="cronExpr" placeholder="0 9 * * *" />
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
        <h3>任务列表</h3>
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
              <td>{{ t.cron_expr || "-" }}</td>
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

.session-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.session-actions {
  display: flex;
  gap: 8px;
}

/* 页面截图区域 */
.screenshot-area {
  margin-top: 14px;
}

.screenshot-frame {
  border: 1px solid var(--border);
  border-radius: var(--radius);
  overflow: auto;
  max-height: 480px;
  background: #fff;
}

.screenshot-frame img {
  display: block;
  width: 100%;
  height: auto;
}

.screenshot-hint {
  font-size: 12px;
  color: var(--text-muted);
  padding: 24px 0;
  text-align: center;
}

.screenshot-tip {
  font-size: 12px;
  color: var(--text-muted);
  margin-top: 8px;
  line-height: 1.6;
}

.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
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
