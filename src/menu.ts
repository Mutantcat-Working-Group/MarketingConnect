// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat

/** 右键菜单项；divider 为 true 时渲染成分隔线，其余字段忽略 */
export interface MenuItem {
  label?: string;
  /** 右侧灰字快捷键提示 */
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  divider?: boolean;
  /** 点击后不关闭菜单（用于展开二次确认等场景） */
  keepOpen?: boolean;
  action?: () => void;
}
