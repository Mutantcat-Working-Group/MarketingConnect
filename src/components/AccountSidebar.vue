<script setup lang="ts">
import { ref, computed } from "vue";
import * as api from "../api";

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
  await api.deleteAccount(id);
  emit("deleted", id);
  emit("refresh");
}

loadPlatforms();
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-header">
      <span>账号管理</span>
      <button class="primary add-btn" @click="showForm = !showForm">
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
      <input v-model="form.group_name" placeholder="分组（可选，如：抖音矩阵A组）" />
      <button class="primary" :disabled="saving" @click="submit">
        {{ saving ? "保存中..." : "保存账号" }}
      </button>
    </div>

    <!-- 账号列表 -->
    <div class="account-list">
      <div v-if="accounts.length === 0" class="empty">暂无账号，点击「+ 新增」添加</div>

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
          @click="emit('select', a.id)"
        >
          <div class="account-info">
            <div class="account-name">{{ a.name }}</div>
            <div class="account-username">{{ a.username }}</div>
          </div>
          <div class="account-meta">
            <span class="badge" :class="a.status">{{ a.status }}</span>
            <button class="del-btn" title="删除账号" @click.stop="remove(a.id)">×</button>
          </div>
        </div>
      </template>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
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
  padding: 9px 10px;
  border-radius: var(--radius);
  cursor: pointer;
  transition: background 0.12s;
}

.account-item:hover {
  background: var(--bg-hover);
}

.account-item.active {
  background: var(--bg-hover);
  box-shadow: inset 2px 0 0 var(--accent);
}

.account-name {
  font-size: 13px;
  font-weight: 500;
}

.account-username {
  font-size: 11px;
  color: var(--text-muted);
}

.account-meta {
  display: flex;
  align-items: center;
  gap: 6px;
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
}
</style>
