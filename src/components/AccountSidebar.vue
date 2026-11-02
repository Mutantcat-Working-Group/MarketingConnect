<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { ref, computed } from "vue";
import * as api from "../api";
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

// 新增账号表单
const form = ref<api.NewAccount>({
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
  platforms.value = await api.listPlatforms();
}

async function submit() {
  if (!form.value.name.trim() || !form.value.username.trim()) {
    return;
  }
  saving.value = true;
  try {
    await api.createAccount({
      platform: form.value.platform,
      name: form.value.name.trim(),
      username: form.value.username.trim(),
      password: form.value.password || undefined,
      proxy_url: form.value.proxy_url || undefined,
      group_name: form.value.group_name?.trim() || undefined,
    });
    form.value = { platform: form.value.platform, name: "", username: "", password: "", proxy_url: "", group_name: "" };
    showForm.value = false;
    emit("refresh");
  } finally {
    saving.value = false;
  }
}

async function remove(id: string) {
  const account = props.accounts.find((a) => a.id === id);
  await api.deleteAccount(id);
  notify(`已删除「${account?.name ?? id}」`);
  emit("deleted", id);
  emit("refresh");
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

const menuItems = computed(() => {
  const a = menu.value?.account;
  if (!a) return [];
  if (confirmDelete.value) {
    return [
      { label: `确认删除「${a.name}」`, danger: true, action: () => remove(a.id) },
      { label: "取消", action: () => (confirmDelete.value = false) },
    ];
  }
  return [
    { label: "打开工作台", shortcut: "Enter", action: () => emit("select", a.id) },
    { label: "检测会话", action: () => checkSession(a) },
    { divider: true },
    { label: "复制备注名", action: () => copyText(a.name) },
    { label: "复制登录账号", action: () => copyText(a.username) },
    ...(a.proxy_url ? [{ label: "复制代理地址", action: () => copyText(a.proxy_url as string) }] : []),
    { divider: true },
    {
      label: "删除账号",
      danger: true,
      keepOpen: true,
      action: () => (confirmDelete.value = true),
    },
  ];
});

/** 供全局快捷键调用：展开新增表单并聚焦第一个输入 */
async function openForm() {
  showForm.value = true;
  await new Promise((r) => setTimeout(r, 0));
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
      <button class="primary add-btn" title="新增账号（Ctrl/⌘ + N）" @click="showForm = !showForm">
        {{ showForm ? "取消" : "+ 新增" }}
      </button>
    </div>

    <!-- 新增账号表单 -->
    <div v-if="showForm" class="add-form">
      <select v-model="form.platform">
        <option v-for="p in platforms" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>
      <input v-model="form.name" placeholder="备注名称" />
      <input v-model="form.username" placeholder="登录账号（手机号/用户名）" />
      <input v-model="form.password" type="password" placeholder="登录密码（加密存储）" />
      <input v-model="form.proxy_url" placeholder="代理地址（可选，如 socks5://...）" />
      <input v-model="form.group_name" placeholder="分组（可选，如：矩阵A组）" />
      <button class="primary" :disabled="saving" @click="submit">
        {{ saving ? "保存中..." : "保存账号" }}
      </button>
    </div>

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
            <div class="account-username">{{ a.username }}</div>
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
