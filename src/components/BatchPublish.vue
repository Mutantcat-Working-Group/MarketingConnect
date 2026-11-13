<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch } from "vue";
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

// 平台 ID -> 平台信息（显示名 + 能力矩阵；如 wechat_mp -> 公众号）
const platforms = ref<Map<string, api.PlatformInfo>>(new Map());

const STATUS_TEXT: Record<string, string> = {
  online: "在线",
  offline: "离线",
  error: "异常",
};

function platformName(id: string): string {
  return platforms.value.get(id)?.name ?? id;
}

/**
 * 安全取平台能力：
 * - 旧后端不返回 `capabilities` → 降级为「只支持文本」，而不是抛错让面板不可用；
 * - 未知平台同样是降级。
 */
function capsOf(platformId: string): api.PlatformCapabilities {
  return api.platformCapabilities(platforms.value.get(platformId));
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

// ---------- 能力驱动：按所选账号的平台能力决定表单可用性 ----------

/** 已选账号 */
const selectedAccounts = computed(() =>
  props.accounts.filter((a) => selected.value.has(a.id))
);

const hasSelection = computed(() => selectedAccounts.value.length > 0);

/** 已选账号涉及的平台能力（按平台去重） */
const selectedCaps = computed(() => {
  const map = new Map<string, api.PlatformCapabilities>();
  for (const a of selectedAccounts.value) {
    map.set(a.platform, capsOf(a.platform));
  }
  return map;
});

/** 在已选平台中筛出「不满足条件」的平台显示名，用于给出可读的原因 */
function platformsFailing(pred: (c: api.PlatformCapabilities) => boolean): string[] {
  return [...selectedCaps.value.entries()]
    .filter(([, c]) => !pred(c))
    .map(([id]) => platformName(id));
}

/** 自动发布尚未实现的平台（如抖音 / 微信公众号）：必须阻止提交 */
const unimplementedPlatforms = computed(() => platformsFailing((c) => c.publish));

/** 不支持文本发布的平台 */
const noTextPlatforms = computed(() => platformsFailing((c) => c.text));

/** 全部选中平台都不支持图片/视频时才禁用媒体输入 */
const mediaSupported = computed(() =>
  [...selectedCaps.value.values()].every((c) => c.images || c.video)
);

/** 未选账号时不限制；选中后按选中平台能力取交集 */
const mediaEnabled = computed(() => !hasSelection.value || mediaSupported.value);

/** 媒体输入被禁用的原因 */
const mediaDisabledReason = computed(() => {
  if (mediaEnabled.value) return "";
  const names = platformsFailing((c) => c.images || c.video);
  return `${names.join("、")} 当前只支持文本发布，无法上传媒体文件`;
});

/** 支持媒体但不支持多文件的平台 */
const multiMediaLimitedPlatforms = computed(() =>
  [...selectedCaps.value.entries()]
    .filter(([, c]) => (c.images || c.video) && !c.multi_image)
    .map(([id]) => platformName(id))
);

/** 已选平台的能力摘要，让用户知道这次发布会带上什么 */
const capabilitySummary = computed(() =>
  [...selectedCaps.value.entries()]
    .map(([id, c]) => {
      const items: string[] = [];
      if (c.text) items.push("文本");
      if (c.images) items.push("图片");
      if (c.video) items.push("视频");
      if (c.topics) items.push("话题");
      if (c.thread) items.push("线程");
      if (c.cover) items.push("封面");
      if (c.schedule) items.push("定时");
      return `${platformName(id)}：${items.length > 0 ? items.join(" / ") : "无可用能力"}`;
    })
    .join("；")
);

/** 不支持自动发布的平台给出的统一提示 */
const UNIMPLEMENTED_HINT =
  "该平台自动发布尚未实现。可点右侧「打开手动发布窗口」，在真实浏览器里自行发布";

/** 打开手动发布窗口的结果提示 */
const manualMessage = ref("");
const manualBusy = ref<string | null>(null);

/**
 * 打开用户可见的浏览器窗口，让用户自己在平台里发布
 *
 * 这是「不可自动发布」平台的正确出路：headless 截图+坐标转发做不了
 * 拖拽、下拉和富文本编辑，真实窗口可以。登录态与工作台共用同一份
 * 加密 cookie，profile 目录独立，不会互相挤掉。
 */
async function openManual(account: api.Account) {
  if (manualBusy.value) return;
  manualBusy.value = account.id;
  manualMessage.value = "";
  try {
    await api.openManualWindow(account.id);
    manualMessage.value = `已为「${account.name}」打开浏览器窗口，请在该窗口中完成发布。`;
  } catch (e) {
    manualMessage.value = `打开窗口失败：${e}`;
  } finally {
    manualBusy.value = null;
  }
}

const unimplementedMessage = computed(() =>
  `${unimplementedPlatforms.value.join("、")}：${UNIMPLEMENTED_HINT}`
);

const canPublish = computed(
  () =>
    selected.value.size > 0 &&
    content.value.trim().length > 0 &&
    !publishing.value &&
    unimplementedPlatforms.value.length === 0 &&
    noTextPlatforms.value.length === 0
);

/** 底栏提示：始终告诉用户下一步该做什么 */
const hint = computed(() => {
  if (props.accounts.length === 0) return "还没有账号，请先在左侧新增账号";
  if (selected.value.size === 0) return "请先勾选要发布的账号";
  if (unimplementedPlatforms.value.length > 0)
    return `${unimplementedPlatforms.value.join("、")} 无法自动发布，请取消选择这些账号`;
  if (noTextPlatforms.value.length > 0)
    return `${noTextPlatforms.value.join("、")} 不支持文本发布，请取消选择这些账号`;
  if (!content.value.trim()) return "请填写正文内容";
  return `将把当前内容发布到 ${selected.value.size} 个账号`;
});

// 媒体输入被禁用时清空已填内容，避免留下「填了却发不出去」的路径
watch(mediaSupported, (ok) => {
  if (ok || !mediaPaths.value.trim()) return;
  mediaPaths.value = "";
  setMessage("err", `${mediaDisabledReason.value}，已自动清空媒体路径`);
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
  // 能力闸门：publish=false 的平台必须阻止提交，不能让用户以为能发
  if (unimplementedPlatforms.value.length > 0) {
    setMessage("err", unimplementedMessage.value);
    return;
  }
  if (noTextPlatforms.value.length > 0) {
    setMessage("err", `${noTextPlatforms.value.join("、")} 不支持文本发布，请取消选择这些账号`);
    return;
  }
  if (!content.value.trim()) {
    setMessage("err", "请输入发布内容");
    return;
  }
  if (mediaPaths.value.trim() && !mediaSupported.value) {
    setMessage("err", `${mediaDisabledReason.value}，请取消选择这些账号`);
    return;
  }
  publishing.value = true;
  setMessage("info", "");
  results.value = [];
  try {
    results.value = await api.batchPublish({
      accountIds: [...selected.value],
      title: title.value || undefined,
      content: content.value,
      mediaPaths: mediaPaths.value || undefined,
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
    platforms.value = new Map(list.map((p) => [p.id, p]));
  } catch {
    // 忽略：回退显示原始平台 ID，并按「只支持文本」降级（capsOf 已兜底）
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
                <span
                  v-if="!capsOf(a.platform).publish"
                  class="cap-tag warn"
                  :title="UNIMPLEMENTED_HINT"
                >
                  不可自动发布
                </span>
                <button
                  v-if="!capsOf(a.platform).publish"
                  class="manual-btn"
                  type="button"
                  :disabled="manualBusy !== null"
                  :title="UNIMPLEMENTED_HINT"
                  @click.prevent.stop="openManual(a)"
                >
                  {{ manualBusy === a.id ? "打开中…" : "打开手动发布窗口" }}
                </button>
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
            <!-- 能力提示：不支持的能力必须给出原因，而不是让用户提交后才失败 -->
            <p v-if="unimplementedPlatforms.length > 0" class="cap-notice err">
              {{ unimplementedMessage }}
            </p>
            <p v-else-if="noTextPlatforms.length > 0" class="cap-notice err">
              {{ noTextPlatforms.join("、") }} 不支持文本发布，请取消选择这些账号
            </p>
            <p v-else-if="hasSelection" class="cap-notice info">
              按平台能力发布 — {{ capabilitySummary }}
            </p>

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
              <input
                v-model="mediaPaths"
                :disabled="!mediaEnabled"
                placeholder="逗号分隔多个文件路径，例如 /a.jpg,/b.mp4"
              />
            </label>
            <p v-if="!mediaEnabled" class="cap-notice err">{{ mediaDisabledReason }}</p>
            <p v-else-if="multiMediaLimitedPlatforms.length > 0" class="cap-notice warn">
              {{ multiMediaLimitedPlatforms.join("、") }} 不支持一次上传多个文件，请只填一个路径
            </p>
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

      <div v-if="manualMessage" class="manual-notice" role="status">
        {{ manualMessage }}
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

/* ---------- 能力提示 ---------- */
.cap-tag {
  flex-shrink: 0;
  padding: 1px 7px;
  font-size: 11px;
  border-radius: 99px;
  white-space: nowrap;
}

.cap-tag.warn {
  color: var(--warning);
  background: rgba(245, 166, 35, 0.12);
}

.cap-notice {
  padding: 7px 10px;
  font-size: 12px;
  line-height: 1.5;
  border-radius: 6px;
  word-break: break-word;
}

.cap-notice.err {
  color: var(--danger);
  background: rgba(240, 82, 79, 0.1);
  border: 1px solid rgba(240, 82, 79, 0.3);
}

.cap-notice.warn {
  color: var(--warning);
  background: rgba(245, 166, 35, 0.1);
  border: 1px solid rgba(245, 166, 35, 0.3);
}

.cap-notice.info {
  color: var(--text-secondary);
  background: var(--bg-secondary);
  border: 1px solid var(--border);
}

input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
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

.manual-btn {
  flex: none;
  padding: 2px 8px;
  font-size: 11px;
  border-radius: 4px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-secondary, #b9bcc4);
  cursor: pointer;
}
.manual-btn:hover:not(:disabled) { border-color: var(--accent, #4f7cff); color: var(--text-primary); }
.manual-btn:disabled { opacity: .5; cursor: default; }

.manual-notice {
  margin: 0 20px 10px;
  padding: 9px 12px;
  border-left: 2px solid var(--success, #3fb950);
  background: rgba(63, 185, 80, .08);
  color: var(--text-secondary, #b9bcc4);
  font-size: 12px;
  line-height: 1.6;
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
