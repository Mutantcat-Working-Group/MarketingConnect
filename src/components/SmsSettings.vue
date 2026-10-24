<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { ref, onMounted } from "vue";
import * as api from "../api";

const config = ref<api.SmsConfigView>({
  access_key_id: "",
  access_key_secret_masked: "",
  has_secret: false,
  sign_name: "",
  template_code: "",
  phone_numbers: "",
  configured: false,
});

const secretInput = ref("");
const saving = ref(false);
const testing = ref(false);
const message = ref("");
const messageType = ref<"ok" | "err">("ok");

async function load() {
  try {
    config.value = await api.getSmsConfig();
  } catch {
    // 忽略
  }
}

async function save() {
  saving.value = true;
  message.value = "";
  try {
    await api.saveSmsConfig({
      access_key_id: config.value.access_key_id,
      // 空 = 保留原密钥
      access_key_secret: secretInput.value,
      sign_name: config.value.sign_name,
      template_code: config.value.template_code,
      phone_numbers: config.value.phone_numbers,
    });
    secretInput.value = "";
    messageType.value = "ok";
    message.value = "配置已保存";
    await load();
  } catch (e) {
    messageType.value = "err";
    message.value = `保存失败: ${e}`;
  } finally {
    saving.value = false;
  }
}

async function test() {
  testing.value = true;
  message.value = "";
  try {
    const result = await api.testSms();
    messageType.value = "ok";
    message.value = result;
  } catch (e) {
    messageType.value = "err";
    message.value = `发送失败: ${e}`;
  } finally {
    testing.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="sms-settings">
    <h3>短信提醒设置</h3>
    <p class="hint">
      掉号时自动发送短信提醒。使用阿里云短信服务，需先在阿里云开通
      SMS 并创建模板（模板变量：<code>account</code>、<code>platform</code>）。
    </p>

    <div class="form">
      <label>
        <span>AccessKey ID</span>
        <input v-model="config.access_key_id" placeholder="LTAI5t..." />
      </label>

      <label>
        <span>AccessKey Secret</span>
        <input
          v-model="secretInput"
          type="password"
          :placeholder="
            config.has_secret
              ? `已保存（${config.access_key_secret_masked}），留空则不修改`
              : '请输入 AccessKey Secret'
          "
        />
      </label>

      <label>
        <span>短信签名</span>
        <input v-model="config.sign_name" placeholder="营销通" />
      </label>

      <label>
        <span>模板 CODE</span>
        <input v-model="config.template_code" placeholder="SMS_123456789" />
      </label>

      <label>
        <span>接收手机号</span>
        <input
          v-model="config.phone_numbers"
          placeholder="13800138000,13900139000"
        />
      </label>
    </div>

    <div class="actions">
      <span v-if="config.configured" class="badge ok">已配置</span>
      <span v-else class="badge off">未配置</span>
      <div class="spacer" />
      <button :disabled="testing || !config.configured" @click="test">
        {{ testing ? "发送中..." : "发送测试" }}
      </button>
      <button class="primary" :disabled="saving" @click="save">
        {{ saving ? "保存中..." : "保存" }}
      </button>
    </div>

    <p v-if="message" class="message" :class="messageType">{{ message }}</p>
  </div>
</template>

<style scoped>
.sms-settings {
  padding: 20px;
  max-width: 560px;
}

h3 {
  margin: 0 0 6px;
  font-size: 15px;
}

.hint {
  margin: 0 0 16px;
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.6;
}

.hint code {
  background: var(--bg-hover);
  padding: 1px 5px;
  border-radius: 3px;
  font-size: 11px;
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
}

.form label span {
  font-size: 12px;
  color: var(--text-secondary);
}

.form input {
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 13px;
}

.form input:focus {
  outline: none;
  border-color: var(--accent, #4a9eff);
}

.actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 16px;
}

.spacer {
  flex: 1;
}

.badge {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 10px;
}

.badge.ok {
  background: rgba(74, 222, 128, 0.15);
  color: #4ade80;
}

.badge.off {
  background: rgba(148, 163, 184, 0.15);
  color: #94a3b8;
}

button {
  padding: 7px 16px;
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
  background: var(--accent, #4a9eff);
  border-color: var(--accent, #4a9eff);
  color: #fff;
}

.message {
  margin-top: 12px;
  font-size: 12px;
}

.message.ok {
  color: #4ade80;
}

.message.err {
  color: var(--danger);
}
</style>
