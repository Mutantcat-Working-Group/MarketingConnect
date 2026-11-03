<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import * as api from "../api";

const config = ref<api.McpConfig>({ enabled: false, port: 8931, token: "", allow_write: false });
const running = ref(false);
const busy = ref(false);
const loading = ref(true);
const message = ref("");
const error = ref(false);
const url = computed(() => `http://127.0.0.1:${config.value.port}/mcp`);
const clientConfig = computed(() => JSON.stringify({
  mcpServers: {
    marketingconnect: {
      url: url.value,
      ...(config.value.token ? { headers: { Authorization: `Bearer ${config.value.token}` } } : {}),
    },
  },
}, null, 2));

async function load() {
  const status = await api.getMcpConfig();
  config.value = status.config;
  running.value = status.running;
}
async function save() {
  busy.value = true;
  message.value = "";
  error.value = false;
  try {
    await api.saveMcpConfig(config.value);
    await load();
    message.value = running.value ? "已保存，MCP 服务正在运行" : "已保存，MCP 服务已关闭";
  } catch (e) {
    error.value = true;
    message.value = String(e);
  } finally { busy.value = false; }
}
async function test() {
  busy.value = true;
  error.value = false;
  try { message.value = await api.testMcp(); }
  catch (e) { error.value = true; message.value = String(e); }
  finally { busy.value = false; }
}
async function copy() {
  try {
    await navigator.clipboard.writeText(clientConfig.value);
    error.value = false;
    message.value = "配置已复制；请将令牌视为密码保管";
  } catch {
    error.value = true;
    message.value = "无法访问剪贴板，请手动选择并复制下方配置";
  }
}
onMounted(async () => {
  try { await load(); }
  catch (e) { error.value = true; message.value = String(e); }
  finally { loading.value = false; }
});
</script>

<template>
  <section class="mcp-settings">
    <div class="heading">
      <div><h3>MCP 接入</h3><p>让支持 HTTP MCP 的 AI 客户端操作营销通。</p></div>
      <span class="badge" :class="running ? 'online' : 'offline'">{{ running ? "运行中" : "未启动" }}</span>
    </div>
    <fieldset :disabled="loading || busy">
      <label class="toggle"><input v-model="config.enabled" type="checkbox" />启用本地 MCP 服务</label>
      <label class="field">监听端口<input v-model.number="config.port" type="number" min="1" max="65535" /></label>
      <label class="field">访问令牌（可选）<input v-model="config.token" type="password" autocomplete="off" placeholder="建议设置令牌，客户端需携带 Bearer 认证" /></label>
      <label class="toggle"><input v-model="config.allow_write" type="checkbox" />允许写入（发布、建号、删号、引擎控制）</label>
      <p class="warning">开启写入后，已连接的 AI 可以直接发布内容和删除账号，无需逐次确认。仅监听 127.0.0.1；营销通退出后服务不可用。</p>
      <div class="actions">
        <button class="primary" @click="save">{{ busy ? "处理中…" : "保存并应用" }}</button>
        <button :disabled="!running" @click="test">测试连接</button>
      </div>
    </fieldset>
    <p class="endpoint">连接地址 <code>{{ url }}</code></p>
    <div class="config-heading"><span>HTTP 客户端配置示例</span><button :disabled="loading" @click="copy">复制配置</button></div>
    <textarea :value="clientConfig" readonly rows="9" aria-label="MCP 客户端 JSON 配置" spellcheck="false" />
    <p class="hint">可用于支持 HTTP URL 的 Cursor 等客户端。不同客户端的配置格式可能不同；只支持 stdio 的客户端需要另行使用桥接器。</p>
    <p v-if="message" class="feedback" :class="{ error }" role="status">{{ message }}</p>
  </section>
</template>

<style scoped>
.mcp-settings { padding: 20px; }
.heading, .config-heading, .actions { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
h3 { font-size: 14px; }
p { font-size: 12px; color: var(--text-secondary); }
.heading { margin-bottom: 16px; }
fieldset { display: flex; flex-direction: column; gap: 12px; border: 0; min-width: 0; }
.field { display: flex; flex-direction: column; gap: 5px; font-size: 12px; color: var(--text-secondary); }
.toggle { display: flex; align-items: center; gap: 8px; font-size: 13px; }
.warning { color: var(--warning); }
.actions { justify-content: flex-start; }
.endpoint { margin: 16px 0; overflow-wrap: anywhere; }
code { color: var(--text-primary); }
.config-heading { font-size: 12px; margin-bottom: 8px; }
textarea { width: 100%; resize: vertical; font-family: ui-monospace, monospace; font-size: 12px; }
.hint { margin-top: 8px; }
.feedback { margin-top: 12px; color: var(--success); overflow-wrap: anywhere; }
.feedback.error { color: var(--danger); }
</style>
