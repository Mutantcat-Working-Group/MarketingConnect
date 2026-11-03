<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { ref, computed, nextTick } from "vue";
import * as api from "../api";
import type { MenuItem } from "../menu";
import ContextMenu from "./ContextMenu.vue";

const props = defineProps<{
  accounts: api.Account[];
  activeId: string | null;
}>();

const emit = defineEmits<{
  select: [id: string];
  refresh: [];
  deleted: [id: string];
}>();

const platforms = ref<api.PlatformInfo[]>([]);
const showForm = ref(false);
const saving = ref(false);
const editingId = ref<string | null>(null);
const formError = ref("");
const clearPassword = ref(false);

// 密码仅用于本次提交，编辑时始终为空。
const form = ref({
  platform: "douyin",
  name: "",
  username: "",
  password: "",
  proxy_url: "",
  group_name: "",
});

// 右键菜单
const menu = ref<{ x: number; y: number; account: api.Account } | null>(null);
const confirmDelete = ref(false);

// 操作反馈
const flash = ref("");
let flashTimer: number | undefined;
function notify(text: string) {
  flash.value = text;
  window.clearTimeout(flashTimer);
  flashTimer = window.setTimeout(() => (flash.value = ""), 2200);
}

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

/** 平台 ID -> 显示名（如 wechat_mp -> 公众号） */
function platformName(id: string): string {
  return platforms.value.find((p) => p.id === id)?.name ?? id;
}

async function loadPlatforms() {
  try {
    platforms.value = await api.listPlatforms();
  } catch {
    notify("加载平台失败，请重试");
  }
}

function closeForm() {
  if (saving.value) return;
  showForm.value = false;
  editingId.value = null;
  form.value.password = "";
  clearPassword.value = false;
  formError.value = "";
}

async function editAccount(account: api.Account) {
  if (saving.value) return;
  editingId.value = account.id;
  form.value = {
    platform: account.platform,
    name: account.name,
    username: account.username,
    password: "",
    proxy_url: account.proxy_url ?? "",
    group_name: account.group_name ?? "",
  };
  clearPassword.value = false;
  formError.value = "";
  showForm.value = true;
  await nextTick();
  document.querySelector<HTMLInputElement>(".add-form input")?.focus();
}

async function submit() {
  formError.value = "";
  if (!form.value.name.trim()) {
    formError.value = "请填写备注名称";
    return;
  }
  if (!editingId.value && !platforms.value.some((p) => p.id === form.value.platform)) {
    formError.value = "请选择可用的平台";
    return;
  }
  if (saving.value) return;
  saving.value = true;
  const wasEditing = editingId.value !== null;
  try {
    const input = {
      name: form.value.name.trim(),
      username: form.value.username.trim(),
      password: clearPassword.value ? undefined : form.value.password || undefined,
      proxy_url: form.value.proxy_url.trim() || undefined,
      group_name: form.value.group_name.trim() || undefined,
    };
    if (editingId.value) {
      await api.updateAccount(editingId.value, { ...input, clear_password: clearPassword.value });
      window.dispatchEvent(new CustomEvent("accounts-changed"));
    } else {
      await api.createAccount({ ...input, platform: form.value.platform });
    }
    form.value = { platform: form.value.platform, name: "", username: "", password: "", proxy_url: "", group_name: "" };
    editingId.value = null;
    clearPassword.value = false;
    showForm.value = false;
    notify(wasEditing ? "账号已更新" : "账号已创建，可打开工作台登录");
    emit("refresh");
  } catch {
    // 不拼接后端错误，避免敏感输入被异常文本回显。
    formError.value = "保存失败，请检查输入后重试";
    emit("refresh");
  } finally {
    form.value.password = "";
    saving.value = false;
  }
}

async function remove(id: string) {
  const account = props.accounts.find((a) => a.id === id);
  try {
    await api.deleteAccount(id);
    if (editingId.value === id) closeForm();
    notify(`已删除「${account?.name ?? id}」`);
    emit("deleted", id);
    emit("refresh");
  } catch {
    notify("删除失败，请重试");
  }
}

/** 复制到剪贴板（webview 下 clipboard API 可能不可用，退回 execCommand） */
async function copyText(text: string) {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
    } else {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    notify("已复制");
  } catch {
    notify("复制失败");
  }
}

/** 检测会话是否有效（掉号判断） */
async function checkSession(account: api.Account) {
  notify("检测中…");
  try {
    const valid = await api.checkAccountSession(account.id);
    notify(valid ? `「${account.name}」会话有效` : `「${account.name}」已掉号`);
    emit("refresh");
  } catch (e) {
    notify(`检测失败：${e}`);
  }
}

function openMenu(e: MouseEvent, account: api.Account) {
  e.preventDefault();
  e.stopPropagation();
  confirmDelete.value = false;
  menu.value = { x: e.clientX, y: e.clientY, account };
}

const menuItems = computed<MenuItem[]>(() => {
  const a = menu.value?.account;
  if (!a) return [];
  if (confirmDelete.value) {
    return [
      { label: `确认删除「${a.name}」`, danger: true, action: () => remove(a.id) },
      { label: "取消", action: () => (confirmDelete.value = false) },
    ];
  }
  const items: MenuItem[] = [
    { label: "打开工作台", shortcut: "Enter", action: () => emit("select", a.id) },
    { label: "检测会话", action: () => checkSession(a) },
    { label: "编辑账号", action: () => editAccount(a) },
    { divider: true },
    { label: "复制备注名", action: () => copyText(a.name) },
    { label: "复制登录账号", action: () => copyText(a.username) },
  ];
  if (a.proxy_url) {
    items.push({ label: "复制代理地址", action: () => copyText(a.proxy_url as string) });
  }
  items.push(
    { divider: true },
    {
      label: "删除账号",
      danger: true,
      keepOpen: true,
      action: () => {
        confirmDelete.value = true;
      },
    }
  );
  return items;
});

/** 供全局快捷键调用：展开新增表单并聚焦第一个输入 */
async function openForm() {
  if (saving.value) return;
  editingId.value = null;
  form.value = { platform: platforms.value.some((p) => p.id === form.value.platform) ? form.value.platform : platforms.value[0]?.id ?? "douyin", name: "", username: "", password: "", proxy_url: "", group_name: "" };
  clearPassword.value = false;
  formError.value = "";
  showForm.value = true;
  await nextTick();
  const first = document.querySelector<HTMLInputElement>(".add-form input");
  first?.focus();
}

async function focusFirstAccount() {
  const list = props.accounts;
  if (list.length > 0) emit("select", list[0].id);
}

defineExpose({ openForm, focusFirstAccount });

loadPlatforms();
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-header">
      <span>账号管理</span>
      <button class="primary add-btn" :disabled="saving" title="新增账号（Ctrl/⌘ + N）" @click="showForm ? closeForm() : openForm()">
        {{ showForm ? "取消" : "+ 新增" }}
      </button>
    </div>

    <!-- 新建/编辑共用表单；从不读取或展示已保存的密码。 -->
    <form v-if="showForm" class="add-form" @submit.prevent="submit">
      <strong class="form-title">{{ editingId ? "编辑账号" : "新增账号" }}</strong>
      <select v-model="form.platform" aria-label="平台" :disabled="!!editingId || saving">
        <option v-if="editingId && !platforms.some((p) => p.id === form.platform)" :value="form.platform">{{ platformName(form.platform) }}</option>
        <option v-for="p in platforms" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>
      <input v-model="form.name" aria-label="备注名称" placeholder="备注名称（必填）" :disabled="saving" required />
      <input v-model="form.username" aria-label="登录账号（可选）" placeholder="登录账号（可选，手机号/用户名）" autocomplete="off" :disabled="saving" />
      <input v-model="form.password" aria-label="登录密码（可选）" type="password" :placeholder="editingId ? '新密码（留空保留原密码）' : '登录密码（可选，加密存储）'" autocomplete="new-password" :disabled="saving || clearPassword" />
      <label v-if="editingId" class="clear-password">
        <input v-model="clearPassword" type="checkbox" :disabled="saving" @change="form.password = ''" />
        清除已保存密码
      </label>
      <input v-model="form.proxy_url" aria-label="代理地址（可选）" placeholder="代理地址（可选，如 socks5://...）" :disabled="saving" />
      <input v-model="form.group_name" aria-label="分组（可选）" placeholder="分组（可选，如：矩阵A组）" :disabled="saving" />
      <p class="form-hint">{{ editingId ? "平台不可更改；修改代理将关闭当前会话。" : "可仅填备注名，创建后在工作台扫码或手动登录。" }}</p>
      <p v-if="formError" class="form-error" role="alert">{{ formError }}</p>
      <button class="primary" type="submit" :disabled="saving">
        {{ saving ? "保存中..." : editingId ? "保存修改" : "保存账号" }}
      </button>
    </form>

    <!-- 账号列表 -->
    <div class="account-list">
      <div v-if="accounts.length === 0" class="empty">
        暂无账号，点击「+ 新增」添加
        <br />
        <span class="empty-key">快捷键 Ctrl/⌘ + N</span>
      </div>

      <template v-for="[group, list] in grouped" :key="group">
        <div class="platform-group">
          <span>{{ group }}</span>
          <span class="group-count">{{ list.length }}</span>
        </div>
        <div
          v-for="a in list"
          :key="a.id"
          class="account-item"
          :class="{ active: a.id === activeId }"
          :title="`${a.name}\n右键查看更多操作`"
          @click="emit('select', a.id)"
          @contextmenu="openMenu($event, a)"
        >
          <div class="account-info">
            <div class="account-name">{{ a.name }}</div>
            <div class="account-username">{{ a.username || "未填写登录账号" }}</div>
          </div>
          <div class="account-meta">
            <span class="platform-tag">{{ platformName(a.platform) }}</span>
            <span class="badge" :class="a.status">{{ a.status }}</span>
            <button class="del-btn" title="删除账号" @click.stop="remove(a.id)">×</button>
          </div>
        </div>
      </template>
    </div>

    <!-- 操作反馈 -->
    <div v-if="flash" class="flash">{{ flash }}</div>

    <!-- 右键菜单 -->
    <ContextMenu
      v-if="menu"
      :x="menu.x"
      :y="menu.y"
      :items="menuItems"
      @close="menu = null"
    />
  </aside>
</template>

<style scoped>
.sidebar {
  position: relative;
  width: 264px;
  min-width: 264px;
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border);
}

.sidebar-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px;
  border-bottom: 1px solid var(--border);
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
}

.add-btn {
  padding: 4px 10px;
  font-size: 12px;
}

.add-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 14px 16px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-tertiary);
}

.form-title {
  font-size: 12px;
  color: var(--text-secondary);
}

.clear-password {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
}

.clear-password input {
  width: auto;
  margin: 0;
}

.form-hint,
.form-error {
  margin: 0;
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-muted);
}

.form-error {
  color: var(--danger);
}

.account-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.platform-group {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 8px 6px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.8px;
}

.group-count {
  font-weight: 400;
  color: var(--text-muted);
  opacity: 0.7;
}

.account-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 9px 10px;
  border-radius: var(--radius);
  cursor: pointer;
  transition: background 0.12s;
  user-select: none;
}

.account-item:hover {
  background: var(--bg-hover);
}

.account-item.active {
  background: var(--bg-hover);
  box-shadow: inset 2px 0 0 var(--accent);
}

.account-info {
  min-width: 0;
}

.account-name {
  font-size: 13px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.account-username {
  font-size: 11px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.account-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.platform-tag {
  padding: 1px 7px;
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 99px;
  white-space: nowrap;
}

.del-btn {
  border: none;
  background: none;
  color: var(--text-muted);
  font-size: 15px;
  padding: 0 4px;
  line-height: 1;
}

.del-btn:hover {
  color: var(--danger);
  background: none;
  border: none;
}

.empty {
  color: var(--text-muted);
  text-align: center;
  padding: 32px 12px;
  font-size: 12px;
  line-height: 2;
}

.empty-key {
  font-size: 11px;
  opacity: 0.7;
}

.flash {
  position: absolute;
  left: 12px;
  right: 12px;
  bottom: 12px;
  padding: 8px 12px;
  font-size: 12px;
  text-align: center;
  color: var(--text-primary);
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  animation: flash-in 0.15s ease-out;
}

@keyframes flash-in {
  from {
    opacity: 0;
    transform: translateY(6px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
</style>
