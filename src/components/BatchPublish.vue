<script setup lang="ts">
import { ref, computed } from "vue";
import * as api from "../api";

const props = defineProps<{ accounts: api.Account[] }>();
const emit = defineEmits<{ close: [] }>();

// 选中的账号
const selected = ref<Set<string>>(new Set());

// 内容表单
const title = ref("");
const content = ref("");
const mediaPaths = ref("");
const publishing = ref(false);
const results = ref<api.BatchPublishResult[]>([]);
const message = ref("");

// 敏感词检测
const checking = ref(false);
const sensitiveResult = ref<api.SensitiveCheckResult | null>(null);

// 按分组展示账号（与侧边栏一致）
const grouped = computed(() => {
  const map = new Map<string, api.Account[]>();
  for (const a of props.accounts) {
    const key = a.group_name?.trim() || "未分组";
    const list = map.get(key) ?? [];
    list.push(a);
    map.set(key, list);
  }
  return map;
});

function toggle(id: string) {
  if (selected.value.has(id)) {
    selected.value.delete(id);
  } else {
    selected.value.add(id);
  }
}

function toggleGroup(list: api.Account[]) {
  const allSelected = list.every((a) => selected.value.has(a.id));
  for (const a of list) {
    if (allSelected) {
      selected.value.delete(a.id);
    } else {
      selected.value.add(a.id);
    }
  }
}

function selectAll() {
  for (const a of props.accounts) {
    selected.value.add(a.id);
  }
}

function clearAll() {
  selected.value.clear();
}

async function publish() {
  if (selected.value.size === 0) {
    message.value = "请先选择账号";
    return;
  }
  if (!content.value.trim()) {
    message.value = "请输入发布内容";
    return;
  }
  publishing.value = true;
  message.value = "";
  results.value = [];
  try {
    results.value = await api.batchPublish({
      account_ids: [...selected.value],
      title: title.value || undefined,
      content: content.value,
      media_paths: mediaPaths.value || undefined,
    });
    const ok = results.value.filter((r) => r.success).length;
    const fail = results.value.length - ok;
    message.value = `发布完成：成功 ${ok} 个，失败 ${fail} 个`;
  } catch (e) {
    message.value = `发布失败: ${e}`;
  } finally {
    publishing.value = false;
  }
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
</script>

<template>
  <div class="modal-overlay" @click.self="emit('close')">
    <div class="modal batch-modal">
      <div class="modal-header">
        <span>批量发布</span>
        <button class="modal-close" @click="emit('close')">×</button>
      </div>
      <div class="modal-body">
        <!-- 账号选择 -->
        <section class="panel">
          <div class="panel-header">
            <h3>选择账号（已选 {{ selected.size }} / {{ accounts.length }}）</h3>
            <div class="panel-actions">
              <button class="small" @click="selectAll">全选</button>
              <button class="small" @click="clearAll">清空</button>
            </div>
          </div>
          <div class="account-picker">
            <div v-for="[group, list] in grouped" :key="group" class="picker-group">
              <label class="group-label">
                <input
                  type="checkbox"
                  :checked="list.every((a) => selected.has(a.id))"
                  @change="toggleGroup(list)"
                />
                <span>{{ group }}（{{ list.length }}）</span>
              </label>
              <div class="group-accounts">
                <label v-for="a in list" :key="a.id" class="account-check">
                  <input
                    type="checkbox"
                    :checked="selected.has(a.id)"
                    @change="toggle(a.id)"
                  />
                  <span class="account-name">{{ a.name }}</span>
                  <span class="account-platform">{{ a.platform }}</span>
                </label>
              </div>
            </div>
          </div>
        </section>

        <!-- 发布内容 -->
        <section class="panel">
          <h3>发布内容</h3>
          <div class="form">
            <input v-model="title" placeholder="标题（可选）" />
            <textarea
              v-model="content"
              rows="5"
              placeholder="发布内容（将发布到所有选中账号）"
            ></textarea>
            <div class="sensitive-check">
              <button
                class="small"
                :disabled="checking || !content.trim()"
                @click="checkWords"
              >
                {{ checking ? "检测中..." : "检测敏感词" }}
              </button>
              <span
                v-if="sensitiveResult"
                class="sensitive-result"
                :class="sensitiveResult.count > 0 ? 'warn' : 'ok'"
              >
                <template v-if="sensitiveResult.count > 0">
                  命中 {{ sensitiveResult.count }} 个：{{ sensitiveResult.hits.join("、") }}
                </template>
                <template v-else>未命中敏感词</template>
              </span>
            </div>
            <input v-model="mediaPaths" placeholder="媒体路径（可选，逗号分隔多个文件）" />
          </div>
          <div class="publish-actions">
            <button
              class="primary"
              :disabled="publishing || selected.size === 0"
              @click="publish"
            >
              {{ publishing ? "发布中..." : `发布到 ${selected.size} 个账号` }}
            </button>
          </div>
          <p v-if="message" class="message">{{ message }}</p>
        </section>

        <!-- 发布结果 -->
        <section v-if="results.length > 0" class="panel">
          <h3>发布结果</h3>
          <table class="result-table">
            <thead>
              <tr>
                <th>账号</th>
                <th>结果</th>
                <th>详情</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="r in results" :key="r.account_id">
                <td>{{ r.account_name ?? r.account_id }}</td>
                <td>
                  <span class="badge" :class="r.success ? 'ok' : 'fail'">
                    {{ r.success ? "成功" : "失败" }}
                  </span>
                </td>
                <td class="error-cell">{{ r.error ?? "—" }}</td>
              </tr>
            </tbody>
          </table>
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
.batch-modal {
  width: 720px;
  max-width: 92vw;
  max-height: 86vh;
}

.modal-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
  overflow-y: auto;
}

.panel {
  padding: 14px;
  background: var(--bg-primary);
  border: 1px solid var(--border);
  border-radius: var(--radius);
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}

h3 {
  margin: 0 0 10px;
  font-size: 14px;
}

.panel-header h3 {
  margin: 0;
}

.panel-actions {
  display: flex;
  gap: 6px;
}

.account-picker {
  max-height: 240px;
  overflow-y: auto;
}

.picker-group {
  margin-bottom: 10px;
}

.group-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  padding: 4px 0;
  cursor: pointer;
}

.group-accounts {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 4px 12px;
  padding-left: 20px;
}

.account-check {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  cursor: pointer;
  padding: 2px 0;
}

.account-name {
  color: var(--text-primary);
}

.account-platform {
  color: var(--text-muted);
  font-size: 11px;
}

.form {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.form input,
.form textarea {
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  resize: vertical;
}

.form input:focus,
.form textarea:focus {
  outline: none;
  border-color: var(--accent);
}

.publish-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 10px;
}

.sensitive-check {
  display: flex;
  align-items: center;
  gap: 10px;
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
  margin-top: 8px;
  font-size: 12px;
  color: var(--text-secondary);
}

.result-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.result-table th {
  text-align: left;
  padding: 6px 8px;
  color: var(--text-muted);
  font-weight: 500;
  border-bottom: 1px solid var(--border);
}

.result-table td {
  padding: 7px 8px;
  border-bottom: 1px solid var(--border);
}

.badge {
  padding: 2px 8px;
  border-radius: 99px;
  font-size: 11px;
}

.badge.ok {
  color: var(--success);
  background: rgba(52, 201, 142, 0.12);
}

.badge.fail {
  color: var(--danger);
  background: rgba(240, 82, 79, 0.12);
}

.error-cell {
  color: var(--text-muted);
  max-width: 260px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

button {
  padding: 7px 14px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: 13px;
  cursor: pointer;
}

button:hover:not(:disabled) {
  background: var(--bg-hover);
}

button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}

button.small {
  padding: 3px 10px;
  font-size: 12px;
}
</style>
