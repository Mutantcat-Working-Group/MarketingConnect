// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mutantcat
import { invoke } from "@tauri-apps/api/core";

export interface McpConfig {
  enabled: boolean;
  port: number;
  token: string;
  allow_write: boolean;
}
export interface McpStatus {
  config: McpConfig;
  running: boolean;
  url: string;
  running_port: number | null;
}
export const getMcpConfig = () => invoke<McpStatus>("get_mcp_config");
export const saveMcpConfig = (config: McpConfig) => invoke<void>("save_mcp_config", { config });
export const testMcp = () => invoke<string>("test_mcp");

/** 账号状态 */
export type AccountStatus = "offline" | "online" | "error";

/** 账号 */
export interface Account {
  id: string;
  platform: string;
  name: string;
  username: string;
  proxy_url: string | null;
  group_name: string | null;
  status: AccountStatus;
  last_check_at: number | null;
  created_at: number;
  updated_at: number;
}

/** 新建账号输入 */
export interface NewAccount {
  platform: string;
  name: string;
  username?: string;
  password?: string;
  proxy_url?: string;
  group_name?: string;
}

export interface UpdateAccount {
  name: string;
  username?: string;
  password?: string;
  clear_password?: boolean;
  proxy_url?: string | null;
  group_name?: string | null;
}
export const updateAccount = (id: string, input: UpdateAccount) =>
  invoke<Account>("update_account", { id, input });
export type LoginAction =
  | { kind: "click"; x: number; y: number }
  | { kind: "scroll"; x: number; y: number; delta: number }
  | { kind: "text"; text: string }
  | { kind: "key"; key: string };
export const loginAction = (accountId: string, action: LoginAction) =>
  invoke<void>("login_action", { accountId, action });
export const setLoginActive = (accountId: string, active: boolean) =>
  invoke<void>("set_login_active", { accountId, active });
export interface LoginResult { authenticated: boolean; saved: boolean; message: string }
export const finishAccountLogin = (accountId: string, force = false) =>
  invoke<LoginResult>("finish_account_login", { accountId, force });

/** 发布任务 */
export interface PublishTask {
  id: string;
  account_id: string;
  title: string | null;
  content: string | null;
  media_paths: string | null;
  cron_expr: string | null;
  status: string;
  result: string | null;
  scheduled_at: number | null;
  executed_at: number | null;
  created_at: number;
}

/** 事件日志 */
export interface EventLog {
  id: number;
  account_id: string | null;
  level: string;
  event: string;
  detail: string | null;
  created_at: number;
}

/** 引擎状态（多实例：按代理分组） */
export interface EngineInstance {
  proxy: string;
  port: number;
  running: boolean;
}

export interface EngineStatus {
  instances: EngineInstance[];
  count: number;
}

/** 平台能力矩阵
 *
 * 字段名与 Rust `platforms::Capabilities` 的 serde 序列化**完全一致**
 * （snake_case），因此这里保持 snake_case，不做 camelCase 转换。
 * 全部为 boolean，语义见 Rust 侧注释：
 * - `publish`：该平台**自动发布是否已实现**（false 表示 publish() 只会报错）；
 * - 其余位表示该平台是否真的读取并处理了对应发布字段。
 */
export interface PlatformCapabilities {
  /** 该平台自动发布是否已实现 */
  publish: boolean;
  text: boolean;
  images: boolean;
  video: boolean;
  multi_image: boolean;
  cover: boolean;
  topics: boolean;
  thread: boolean;
  schedule: boolean;
}

/** 平台信息
 *
 * 后三个字段是后端新增的，设为可选以兼容旧后端（旧后端只返回 id/name）。
 */
export interface PlatformInfo {
  id: string;
  name: string;
  /** 该平台是否支持 API 令牌登录（后端 `token_hint()` 非空） */
  tokenSupported?: boolean;
  /** 令牌获取位置提示；浏览器族平台为 null */
  tokenHint?: string | null;
  /** 平台能力矩阵；旧后端不返回该字段 */
  capabilities?: PlatformCapabilities;
}

/** 读不到能力矩阵时的降级：按「只支持文本发布」处理
 *
 * 旧后端不返回 `capabilities`，若直接解引用可选字段会抛错并导致整个面板不可用。
 * `publish: true` 表示仍按可实现发布处理（降级而非禁用），与 Rust 的
 * `Capabilities::TEXT_ONLY` 保持一致。
 */
export const TEXT_ONLY_CAPABILITIES: PlatformCapabilities = {
  publish: true,
  text: true,
  images: false,
  video: false,
  multi_image: false,
  cover: false,
  topics: false,
  thread: false,
  schedule: false,
};

/** 安全地取平台能力：缺字段 / 平台未知时降级为「只支持文本」 */
export function platformCapabilities(
  info: PlatformInfo | undefined | null
): PlatformCapabilities {
  return info?.capabilities ?? TEXT_ONLY_CAPABILITIES;
}

// ---------- 账号 ----------

export const listAccounts = () => invoke<Account[]>("list_accounts");

export const createAccount = (input: NewAccount) =>
  invoke<Account>("create_account", { input });

export const deleteAccount = (id: string) =>
  invoke<void>("delete_account", { id });

// ---------- 引擎 ----------

export const startEngine = () => invoke<string>("start_engine");

export const stopEngine = () => invoke<void>("stop_engine");

export const engineStatus = () => invoke<EngineStatus>("engine_status");

// ---------- 会话检测 ----------

export interface AccountSessionStatus { open: boolean; login_active: boolean }
export const accountSessionStatus = (accountId: string) =>
  invoke<AccountSessionStatus>("account_session_status", { accountId });
export const openAccountSession = (accountId: string) =>
  invoke<void>("open_account_session", { accountId });

export const closeAccountSession = (accountId: string) =>
  invoke<void>("close_account_session", { accountId });

export const checkAccountSession = (accountId: string) =>
  invoke<boolean>("check_account_session", { accountId });

/**
 * 打开一个用户可见、可直接操作的浏览器窗口（手动发布用）
 *
 * 平台未实现自动发布时，用户在真实 Chrome 窗口里自行完成发布。
 * 登录态与工作台会话共用同一份加密 cookie，profile 目录独立。
 */
export const openManualWindow = (accountId: string) =>
  invoke<number>("open_manual_window", { accountId });

export const saveAccountCookies = (accountId: string) =>
  invoke<number>("save_account_cookies", { accountId });

export const captureScreenshot = (accountId: string) =>
  invoke<string>("capture_screenshot", { accountId });

// ---------- 发布任务 ----------

/**
 * 创建发布任务
 *
 * 注意：Tauri 2 默认把 Rust 命令参数名转为 camelCase 作为 IPC 键，
 * 因此这里必须是 camelCase（`accountId` / `mediaPaths` / ...）；
 * 传 snake_case 会在运行时直接报「缺少参数」。
 */
export const createPublishTask = (params: {
  accountId: string;
  title?: string;
  content?: string;
  mediaPaths?: string;
  cronExpr?: string;
  /** 单位：秒（与数据库 scheduled_at 列一致） */
  scheduledAt?: number;
  /** 封面图路径；前端控件留到 Task 11 的能力 UI */
  coverPath?: string;
  /** 话题标签，以 `,` 分隔存储 */
  topics?: string;
  /** 线程分段，以 `||` 分隔存储 */
  threadSegments?: string;
}) => invoke<PublishTask>("create_publish_task", params);

export const listPublishTasks = (accountId: string) =>
  invoke<PublishTask[]>("list_publish_tasks", { accountId });

/** 批量发布单账号结果 */
export interface BatchPublishResult {
  account_id: string;
  account_name: string | null;
  success: boolean;
  error: string | null;
}

/**
 * 批量发布：同一内容发布到多个账号
 *
 * 键名同样必须是 camelCase（Rust 侧为 `account_ids` / `media_paths`）。
 */
export const batchPublish = (params: {
  accountIds: string[];
  title?: string;
  content: string;
  mediaPaths?: string;
}) => invoke<BatchPublishResult[]>("batch_publish", params);

// ---------- 日志与平台 ----------

export const recentLogs = (limit?: number) =>
  invoke<EventLog[]>("recent_logs", { limit });

export const listPlatforms = () => invoke<PlatformInfo[]>("list_platforms");

// ---------- 平台 API 令牌（按账号存储） ----------

/**
 * 保存并校验平台 API 令牌（仅令牌型平台）
 *
 * 后端会先调用 `verify_token` 校验令牌有效性，失败时抛出可直接展示的错误，
 * 因此调用方必须把错误信息显示给用户。
 *
 * Rust 参数为 `account_id` / `token`，按 Tauri 2 的 camelCase 规则，JS 键为
 * `accountId` / `token`。
 */
export const savePlatformToken = (accountId: string, token: string) =>
  invoke<void>("save_platform_token", { accountId, token });

/**
 * 查询账号是否已配置 API 令牌
 *
 * 后端只检查密文列是否存在，不解密、不触发系统钥匙串。
 * 账号不存在时会抛错（而不是返回 false），调用方需自行处理。
 */
export const apiTokenConfigured = (accountId: string) =>
  invoke<boolean>("api_token_configured", { accountId });

// ---------- 短信配置 ----------

/** 短信配置（密钥脱敏） */
export interface SmsConfigView {
  access_key_id: string;
  access_key_secret_masked: string;
  has_secret: boolean;
  sign_name: string;
  template_code: string;
  phone_numbers: string;
  configured: boolean;
}

export const getSmsConfig = () => invoke<SmsConfigView>("get_sms_config");

/**
 * 保存短信配置
 *
 * Rust 命令 `save_sms_config` 的参数是顶层参数（`access_key_id` 等），按 Tauri 2
 * 的 camelCase 规则，JS 侧键名必须是 `accessKeyId` / `accessKeySecret` / `signName`
 * / `templateCode` / `phoneNumbers`；传 snake_case 会在运行时直接报「缺少参数」。
 *
 * 注意这与 `SmsConfigView`（`get_sms_config` 的**返回值**）不同：返回值由 Rust
 * 直接序列化为 snake_case，保持不变。
 */
export const saveSmsConfig = (params: {
  accessKeyId: string;
  accessKeySecret: string;
  signName: string;
  templateCode: string;
  phoneNumbers: string;
}) => invoke<void>("save_sms_config", params);

export const testSms = () => invoke<string>("test_sms");

// ---------- 自定义平台 ----------

/** 自定义平台配置 */
export interface CustomPlatform {
  id: string;
  name: string;
  creator_url: string;
  publish_url: string;
  title_selector: string | null;
  content_selector: string | null;
  submit_selector: string | null;
  logged_in_pattern: string | null;
}

export const listCustomPlatforms = () =>
  invoke<CustomPlatform[]>("list_custom_platforms");

/**
 * 新增自定义平台
 *
 * Rust 命令 `create_custom_platform` 的参数是顶层参数（`creator_url` 等），按
 * Tauri 2 的 camelCase 规则，JS 侧键名必须是 `creatorUrl` / `publishUrl` /
 * `titleSelector` / `contentSelector` / `submitSelector` / `loggedInPattern`；
 * 传 snake_case 会在运行时直接报「缺少参数」。
 *
 * 返回的 `CustomPlatform`（`list_custom_platforms`）是 Rust 序列化结果，仍为
 * snake_case，保持不变。
 */
export const createCustomPlatform = (params: {
  id: string;
  name: string;
  creatorUrl: string;
  publishUrl: string;
  titleSelector?: string;
  contentSelector?: string;
  submitSelector?: string;
  loggedInPattern?: string;
}) => invoke<void>("create_custom_platform", params);

export const deleteCustomPlatform = (id: string) =>
  invoke<void>("delete_custom_platform", { id });

// ---------- 敏感词检测 ----------

/** 敏感词检测结果 */
export interface SensitiveCheckResult {
  hits: string[];
  count: number;
  word_count: number;
}

/** 检测文本中的敏感词 */
export const checkSensitiveWords = (text: string) =>
  invoke<SensitiveCheckResult>("check_sensitive_words", { text });
