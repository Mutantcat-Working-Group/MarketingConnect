<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- Copyright (C) 2026 Mutantcat -->
<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, nextTick } from "vue";
import type { MenuItem } from "../menu";

const props = defineProps<{ x: number; y: number; items: MenuItem[] }>();
const emit = defineEmits<{ close: [] }>();

const el = ref<HTMLElement | null>(null);
const pos = ref({ left: props.x, top: props.y });

function close() {
  emit("close");
}

function pick(item: MenuItem) {
  if (item.disabled || item.divider || !item.action) return;
  item.action();
  if (!item.keepOpen) close();
}

/** 点击菜单外部关闭 */
function onPointerDown(e: MouseEvent) {
  if (el.value && !el.value.contains(e.target as Node)) close();
}

/** Esc 关闭（捕获阶段，优先于全局快捷键） */
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    close();
  }
}

/** 菜单贴近视口边缘时向内收，避免被裁掉 */
async function place() {
  await nextTick();
  const rect = el.value?.getBoundingClientRect();
  if (!rect) return;
  const pad = 8;
  let left = props.x;
  let top = props.y;
  if (left + rect.width > window.innerWidth - pad) {
    left = Math.max(pad, window.innerWidth - rect.width - pad);
  }
  if (top + rect.height > window.innerHeight - pad) {
    top = Math.max(pad, window.innerHeight - rect.height - pad);
  }
  pos.value = { left, top };
}

onMounted(() => {
  place();
  document.addEventListener("mousedown", onPointerDown, true);
  document.addEventListener("keydown", onKey, true);
  window.addEventListener("blur", close);
  window.addEventListener("resize", close);
});

onBeforeUnmount(() => {
  document.removeEventListener("mousedown", onPointerDown, true);
  document.removeEventListener("keydown", onKey, true);
  window.removeEventListener("blur", close);
  window.removeEventListener("resize", close);
});
</script>

<template>
  <div
    ref="el"
    class="context-menu"
    :style="{ left: `${pos.left}px`, top: `${pos.top}px` }"
    @contextmenu.prevent
  >
    <template v-for="(item, i) in items" :key="i">
      <div v-if="item.divider" class="ctx-divider" />
      <button
        v-else
        class="ctx-item"
        :class="{ danger: item.danger }"
        :disabled="item.disabled"
        @click="pick(item)"
      >
        <span class="ctx-label">{{ item.label }}</span>
        <span v-if="item.shortcut" class="ctx-key">{{ item.shortcut }}</span>
      </button>
    </template>
  </div>
</template>

<style scoped>
.context-menu {
  position: fixed;
  z-index: 300;
  min-width: 184px;
  padding: 5px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.5);
  animation: ctx-in 0.1s ease-out;
}

.ctx-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
  width: 100%;
  padding: 7px 10px;
  font-size: 13px;
  color: var(--text-primary);
  text-align: left;
  background: transparent;
  border: none;
  border-radius: 6px;
}

.ctx-item:hover:not(:disabled) {
  background: var(--bg-hover);
  border-color: transparent;
}

.ctx-item.danger {
  color: var(--danger);
}

.ctx-item.danger:hover:not(:disabled) {
  background: rgba(240, 82, 79, 0.14);
}

.ctx-item:disabled {
  opacity: 0.4;
}

.ctx-key {
  font-size: 11px;
  color: var(--text-muted);
}

.ctx-divider {
  height: 1px;
  margin: 5px 6px;
  background: var(--border);
}

@keyframes ctx-in {
  from {
    opacity: 0;
    transform: scale(0.97);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
</style>
