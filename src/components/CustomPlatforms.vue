<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { ref, onMounted } from "vue";
import * as api from "../api";

const platforms = ref<api.CustomPlatform[]>([]);
const loading = ref(false);
const message = ref("");
const messageType = ref<"ok" | "err">("ok");

// 新平台表单
const form = ref({
  id: "",
  name: "",
  creator_url: "",
  publish_url: "",
  title_selector: "",
  content_selector: "",
  submit_selector: "",
  logged_in_pattern: "",
});
const saving = ref(false);
const showForm = ref(false);

async function load() {
  loading.value = true;
  try {
    platforms.value = await api.listCustomPlatforms();
  } catch {
    // 忽略
  } finally {
    loading.value = false;
  }
}

async function save() {
  if (!form.value.id.trim() || !form.value.name.trim()) {
    messageType.value = "err";
    message.value = "请填写平台 ID 和名称";
    return;
  }
  if (!form.value.creator_url.trim() || !form.value.publish_url.trim()) {
    messageType.value = "err";
    message.value = "请填写创作者首页和发布页 URL";
    return;
  }
  saving.value = true;
  message.value = "";
  try {
    await api.createCustomPlatform({
      id: form.value.id.trim(),
      name: form.value.name.trim(),
      creator_url: form.value.creator_url.trim(),
      publish_url: form.value.publish_url.trim(),
      title_selector: form.value.title_selector.trim() || undefined,
      content_selector: form.value.content_selector.trim() || undefined,
      submit_selector: form.value.submit_selector.trim() || undefined,
      logged_in_pattern: form.value.logged_in_pattern.trim() || undefined,
    });
    messageType.value = "ok";
    message.value = "平台已创建";
    resetForm();
    await load();
  } catch (e) {
    messageType.value = "err";
    message.value = `创建失败: ${e}`;
  } finally {
    saving.value = false;
  }
}

async function remove(id: string) {
  try {
    await api.deleteCustomPlatform(id);
    messageType.value = "ok";
    message.value = "已删除";
    await load();
  } catch (e) {
    messageType.value = "err";
    message.value = `删除失败: ${e}`;
  }
}

function resetForm() {
  form.value = {
    id: "",
    name: "",
    creator_url: "",
    publish_url: "",
    title_selector: "",
    content_selector: "",
    submit_selector: "",
    logged_in_pattern: "",
  };
  showForm.value = false;
}

onMounted(load);
</script>

<template>
  <div class="custom-platforms">
    <div class="section-header">
      <h3>自定义平台</h3>
      <button class="primary" @click="showForm = !showForm">
        {{ showForm ? "取消" : "+ 新增平台" }}
      </button>
    </div>
    <p class="hint">
      注册内置平台之外的自定义平台。登录通过截图辅助手动完成；
      发布按选择器自动填充表单（选择器可留空，留空则只导航不填充）。
    </p>

    <!-- 新增表单 -->
    <div v-if="showForm" class="form">
      <div class="form-row">
        <label>
          <span>平台 ID（英文，如 my-platform）*</span>
          <input v-model="form.id" placeholder="my-platform" />
        </label>
        <label>
          <span>平台名称 *</span>
          <input v-model="form.name" placeholder="我的平台" />
        </label>
      </div>
      <label>
        <span>创作者首页 URL *</span>
        <input v-model="form.creator_url" placeholder="https://creator.example.com/home" />
      </label>
      <label>
        <span>发布页 URL *</span>
        <input v-model="form.publish_url" placeholder="https://creator.example.com/publish" />
      </label>
      <div class="form-row">
        <label>
          <span>标题选择器（可选）</span>
          <input v-model="form.title_selector" placeholder="#title 或 .title-input" />
        </label>
        <label>
          <span>内容选择器（可选）</span>
          <input v-model="form.content_selector" placeholder=".editor 或 textarea" />
        </label>
      </div>
      <div class="form-row">
        <label>
          <span>提交按钮选择器（可选）</span>
          <input v-model="form.submit_selector" placeholder="button[type=submit]" />
        </label>
        <label>
          <span>登录态 URL 特征（可选）</span>
          <input v-model="form.logged_in_pattern" placeholder="如 /home（登录后 URL 包含）" />
        </label>
      </div>
      <div class="form-actions">
        <button :disabled="saving" @click="save">
          {{ saving ? "保存中..." : "保存" }}
        </button>
      </div>
    </div>

    <!-- 平台列表 -->
    <table v-if="platforms.length > 0" class="platform-table">
      <thead>
        <tr>
          <th>ID</th>
          <th>名称</th>
          <th>创作者首页</th>
          <th>发布页</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="p in platforms" :key="p.id">
          <td class="mono">{{ p.id }}</td>
          <td>{{ p.name }}</td>
          <td class="url">{{ p.creator_url }}</td>
          <td class="url">{{ p.publish_url }}</td>
          <td>
            <button class="danger small" @click="remove(p.id)">删除</button>
          </td>
        </tr>
      </tbody>
    </table>
    <p v-else-if="!loading" class="empty">暂无自定义平台</p>

    <p v-if="message" class="message" :class="messageType">{{ message }}</p>
  </div>
</template>

<style scoped>
.custom-platforms {
  padding: 0 4px;
}

.section-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 6px;
}

h3 {
  margin: 0;
  font-size: 15px;
}

.hint {
  margin: 0 0 14px;
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.6;
}

.form {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  background: var(--bg-primary);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  margin-bottom: 14px;
}

.form-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.form label {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.form label span {
  font-size: 12px;
  color: var(--text-secondary);
}

.form input {
  padding: 7px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: 13px;
}

.form input:focus {
  outline: none;
  border-color: var(--accent);
}

.form-actions {
  display: flex;
  justify-content: flex-end;
}

.platform-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.platform-table th {
  text-align: left;
  padding: 7px 8px;
  color: var(--text-muted);
  font-weight: 500;
  border-bottom: 1px solid var(--border);
}

.platform-table td {
  padding: 8px;
  border-bottom: 1px solid var(--border);
}

.mono {
  font-family: ui-monospace, monospace;
  color: var(--text-secondary);
}

.url {
  color: var(--text-muted);
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.empty {
  color: var(--text-muted);
  text-align: center;
  padding: 20px 0;
  font-size: 13px;
}

.message {
  margin-top: 10px;
  font-size: 12px;
}

.message.ok {
  color: var(--success);
}

.message.err {
  color: var(--danger);
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

button.danger {
  color: var(--danger);
  border-color: var(--danger);
}

button.small {
  padding: 3px 10px;
  font-size: 12px;
}
</style>
