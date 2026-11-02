<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from "vue";
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
const messageKind = ref<"info" | "ok" | "err">("info");

// 敏感词检测
const checking = ref(false);
const sensitiveResult = ref<api.SensitiveCheckResult | null>(null);

// 平台 ID -> 显示名（如 wechat_mp -> 公众号）
const platformNames = ref<Map<string, string>>(new Map());

const STATUS_TEXT: Record<string, string> = {
  online: "在线",
  offline: "离线",
  error: "异常",
};

function platformName(id: string): string {
  return platformNames.value.get(id) ?? id;
}

function statusText(status: string): string {
  return STATUS_TEXT[status] ?? status;
}

function setMessage(kind: "info" | "ok" | "err", text: string) {
  messageKind.value = kind;
  message.value = text;
}

// 按分组展示账号
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

const successCount = computed(() => results.value.filter((r) => r.success).length);

const canPublish = computed(
  () => selected.value.size > 0 && content.value.trim().length > 0 && !publishing.value
);

/** 底栏提示：始终告诉用户下一步该做什么 */
const hint = computed(() => {
  if (props.accounts.length === 0) return "还没有账号，请先在左侧新增账号";
  if (selected.value.size === 0) return "请先勾选要发布的账号";
  if (!content.value.trim()) return "请填写正文内容";
  return `将把当前内容发布到 ${selected.value.size} 个账号`;
});

function groupStats(list: api.Account[]) {
  const count = list.filter((a) => selected.value.has(a.id)).length;
  return { count, all: count === list.length && count > 0, partial: count > 0 && count < list.length };
}

function toggle(id: string) {
  if (selected.value.has(id)) {
    selected.value.delete(id);
  } else {
    selected.value.add(id);
  }
}

function toggleGroup(list: api.Account[]) {
  const { all } = groupStats(list);
  for (const a of list) {
    if (all) {
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
    setMessage("err", "请先选择账号");
    return;
  }
  if (!content.value.trim()) {
    setMessage("err", "请输入发布内容");
    return;
  }
  publishing.value = true;
  setMessage("info", "");
  results.value = [];
  try {
    results.value = await api.batchPublish({
      account_ids: [...selected.value],
      title: title.value || undefined,
      content: content.value,
      media_paths: mediaPaths.value || undefined,
    });
    const ok = successCount.value;
    const fail = results.value.length - ok;
    setMessage(fail === 0 ? "ok" : "err", `发布完成：成功 ${ok} 个，失败 ${fail} 个`);
  } catch (e) {
    setMessage("err", `发布失败: ${e}`);
  } finally {
    publishing.value = false;
  }
}

async function checkWords() {
  checking.value = true;
  setMessage("info", "");
  try {
    sensitiveResult.value = await api.checkSensitiveWords(content.value);
  } catch (e) {
    setMessage("err", `检测失败: ${e}`);
  } finally {
    checking.value = false;
  }
}

/** Ctrl/⌘ + Enter 直接提交 */
function onKey(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
    e.preventDefault();
    if (canPublish.value) publish();
  }
}

onMounted(async () => {
  document.addEventListener("keydown", onKey);
  try {
    const list = await api.listPlatforms();
    platformNames.value = new Map(list.map((p) => [p.id, p.name]));
  } catch {
    // 忽略：回退显示原始平台 ID
  }
});

onBeforeUnmount(() => {
  document.removeEventListener("keydown", onKey);
});
</script>

<template>
  <div class="modal-overlay" @click.self="emit('close')">
    <div class="modal batch-modal">
      <div class="modal-header">
        <div>
          <div class="modal-title">批量发布</div>
          <div class="modal-subtitle">同一内容一次分发到多个账号</div>
        </div>
        <button class="modal-close" aria-label="关闭批量发布" @click="emit('close')">×</button>
      </div>

      <div class="modal-body">
        <!-- 1. 选择账号 -->
        <section class="step">
          <div class="step-head">
            <span class="step-no">1</span>
            <h3 class="step-title">选择账号</h3>
            <span class="count-pill" :class="{ on: selected.size > 0 }">
              {{ selected.size }} / {{ accounts.length }}
            </span>
            <div class="step-spacer" />
            <button class="ghost" :disabled="accounts.length === 0" @click="selectAll">
              全选
            </button>
            <button class="ghost" :disabled="selected.size === 0" @click="clearAll">
              清空
            </button>
          </div>

          <div v-if="accounts.length === 0" class="empty-state">
            <p>还没有可发布的账号</p>
            <p class="empty-hint">请先在左侧「+ 新增」添加账号（Ctrl/⌘ + N）</p>
          </div>

          <div v-else class="account-picker">
            <div v-for="[group, list] in grouped" :key="group" class="picker-group">
              <label class="group-row">
                <input
                  type="checkbox"
                  :checked="groupStats(list).all"
                  :indeterminate="groupStats(list).partial"
                  @change="toggleGroup(list)"
                />
                <span class="group-name">{{ group }}</span>
                <span class="group-count">{{ groupStats(list).count }}/{{ list.length }}</span>
              </label>
              <label
                v-for="a in list"
                :key="a.id"
                class="account-row"
                :class="{ on: selected.has(a.id) }"
              >
                <input type="checkbox" :checked="selected.has(a.id)" @change="toggle(a.id)" />
                <span class="account-name">{{ a.name }}</span>
                <span class="account-username">{{ a.username }}</span>
                <span class="platform-tag">{{ platformName(a.platform) }}</span>
                <span class="badge" :class="a.status">{{ statusText(a.status) }}</span>
              </label>
            </div>
          </div>
        </section>

        <!-- 2. 发布内容 -->
        <section class="step">
          <div class="step-head">
            <span class="step-no">2</span>
            <h3 class="step-title">发布内容</h3>
            <div class="step-spacer" />
            <span class="meta-count">{{ content.length }} 字</span>
          </div>

          <div class="fields">
            <label class="field">
              <span class="field-label">标题 <em>可选</em></span>
              <input v-model="title" placeholder="留空时由平台自动取正文首行" />
            </label>

            <label class="field">
              <span class="field-label">正文 <em>必填</em></span>
              <textarea v-model="content" rows="6" placeholder="输入要发布的正文内容…" />
            </label>

            <div class="sensitive-row">
              <button class="ghost" :disabled="checking || !content.trim()" @click="checkWords">
                {{ checking ? "检测中…" : "检测敏感词" }}
              </button>
              <span
                v-if="sensitiveResult"
                class="sensitive-result"
                :class="sensitiveResult.count > 0 ? 'warn' : 'ok'"
              >
                <template v-if="sensitiveResult.count > 0">
                  命中 {{ sensitiveResult.count }} 个：{{ sensitiveResult.hits.join("、") }}
                </template>
                <template v-else>
                  未命中敏感词（已扫描 {{ sensitiveResult.word_count }} 条词库）
                </template>
              </span>
            </div>

            <label class="field">
              <span class="field-label">媒体路径 <em>可选</em></span>
              <input v-model="mediaPaths" placeholder="逗号分隔多个文件路径，例如 /a.jpg,/b.mp4" />
            </label>
          </div>
        </section>

        <!-- 3. 发布结果 -->
        <section v-if="results.length > 0" class="step">
          <div class="step-head">
            <span class="step-no">3</span>
            <h3 class="step-title">发布结果</h3>
            <div class="step-spacer" />
            <span class="meta-count">
              {{ successCount }} 成功 / {{ results.length - successCount }} 失败
            </span>
          </div>
          <div class="result-list">
            <div v-for="r in results" :key="r.account_id" class="result-row">
              <span class="badge" :class="r.success ? 'ok' : 'fail'">
                {{ r.success ? "成功" : "失败" }}
              </span>
              <span class="result-name">{{ r.account_name ?? r.account_id.slice(0, 8) }}</span>
              <span v-if="r.error" class="result-error" :title="r.error">{{ r.error }}</span>
            </div>
          </div>
        </section>
      </div>

      <div class="modal-footer">
        <span class="footer-msg" :class="messageKind">{{ message || hint }}</span>
        <button class="primary publish-btn" :disabled="!canPublish" @click="publish">
          {{ publishing ? "发布中…" : `发布到 ${selected.size} 个账号` }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.batch-modal {
  width: 760px;
}

.batch-modal .modal-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 16px 18px;
}

/* ---------- 步骤卡片 ---------- */
.step {
  padding: 13px 14px;
  background: var(--bg-primary);
  border: 1px solid var(--border);
  border-radius: 10px;
}

.step-head {
  display: flex;
  align-items: center;
  gap: 9px;
}

.step-no {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 19px;
  height: 19px;
  font-size: 11px;
  font-weight: 600;
  color: #fff;
  background: var(--accent);
  border-radius: 6px;
}

.step-title {
  font-size: 13px;
  font-weight: 600;
}

.step-spacer {
  flex: 1;
}

.count-pill {
  padding: 1px 9px;
  font-size: 11.5px;
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 99px;
  transition: all 0.15s;
}

.count-pill.on {
  color: var(--accent);
  background: rgba(79, 124, 255, 0.12);
  border-color: rgba(79, 124, 255, 0.4);
}

.meta-count {
  font-size: 11.5px;
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
}

.ghost {
  padding: 4px 10px;
  font-size: 12px;
}

/* ---------- 账号选择 ---------- */
.account-picker {
  max-height: 208px;
  margin-top: 10px;
  padding: 5px;
  overflow-y: auto;
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 8px;
}

.picker-group + .picker-group {
  margin-top: 3px;
}

.group-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  font-size: 11.5px;
  color: var(--text-muted);
  cursor: pointer;
  user-select: none;
}

.group-name {
  font-weight: 600;
  letter-spacing: 0.6px;
}

.group-count {
  margin-left: auto;
  font-variant-numeric: tabular-nums;
  opacity: 0.75;
}

.account-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 8px 7px 24px;
  border-radius: 6px;
  cursor: pointer;
  user-select: none;
  transition: background 0.12s;
}

.account-row:hover {
  background: var(--bg-hover);
}

.account-row.on {
  background: rgba(79, 124, 255, 0.1);
}

.account-name {
  font-size: 13px;
  white-space: nowrap;
}

.account-username {
  min-width: 0;
  font-size: 11.5px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.platform-tag {
  margin-left: auto;
  padding: 1px 7px;
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 99px;
  white-space: nowrap;
}

.empty-state {
  padding: 26px 12px;
  margin-top: 10px;
  font-size: 13px;
  color: var(--text-secondary);
  text-align: center;
  background: var(--bg-secondary);
  border: 1px dashed var(--border);
  border-radius: 8px;
}

.empty-hint {
  margin-top: 5px;
  font-size: 11.5px;
  color: var(--text-muted);
}

/* ---------- 内容表单 ---------- */
.fields {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 12px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.field-label {
  font-size: 12px;
  color: var(--text-secondary);
}

.field-label em {
  margin-left: 4px;
  font-size: 11px;
  font-style: normal;
  color: var(--text-muted);
}

textarea {
  resize: vertical;
  line-height: 1.6;
}

.sensitive-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}

.sensitive-result {
  font-size: 12px;
}

.sensitive-result.ok {
  color: var(--success);
}

.sensitive-result.warn {
  color: var(--warning);
}

/* ---------- 结果 ---------- */
.result-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 168px;
  margin-top: 10px;
  overflow-y: auto;
}

.result-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 0;
  font-size: 12.5px;
}

.result-name {
  white-space: nowrap;
}

.result-error {
  min-width: 0;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---------- 底栏 ---------- */
.footer-msg {
  font-size: 12px;
  color: var(--text-muted);
}

.footer-msg.ok {
  color: var(--success);
}

.footer-msg.err {
  color: var(--danger);
}

.publish-btn {
  padding: 8px 18px;
  font-weight: 500;
}

.badge.ok {
  color: var(--success);
  background: rgba(52, 201, 142, 0.12);
}

.badge.fail {
  color: var(--danger);
  background: rgba(240, 82, 79, 0.12);
}
</style>
