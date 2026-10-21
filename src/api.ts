import { invoke } from "@tauri-apps/api/core";

/** 账号状态 */
export type AccountStatus = "offline" | "online" | "error";

/** 账号 */
export interface Account {
  id: string;
  platform: string;
  name: string;
  username: string;
  proxy_url: string | null;
  status: AccountStatus;
  last_check_at: number | null;
  created_at: number;
  updated_at: number;
}

/** 新建账号输入 */
export interface NewAccount {
  platform: string;
  name: string;
  username: string;
  password?: string;
  proxy_url?: string;
}

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

/** 平台信息 */
export interface PlatformInfo {
  id: string;
  name: string;
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

export const openAccountSession = (accountId: string) =>
  invoke<void>("open_account_session", { accountId });

export const closeAccountSession = (accountId: string) =>
  invoke<void>("close_account_session", { accountId });

export const checkAccountSession = (accountId: string) =>
  invoke<boolean>("check_account_session", { accountId });

export const saveAccountCookies = (accountId: string) =>
  invoke<number>("save_account_cookies", { accountId });

export const captureScreenshot = (accountId: string) =>
  invoke<string>("capture_screenshot", { accountId });

// ---------- 发布任务 ----------

export const createPublishTask = (params: {
  account_id: string;
  title?: string;
  content?: string;
  media_paths?: string;
  cron_expr?: string;
  scheduled_at?: number;
}) => invoke<PublishTask>("create_publish_task", params);

export const listPublishTasks = (accountId: string) =>
  invoke<PublishTask[]>("list_publish_tasks", { accountId });

// ---------- 日志与平台 ----------

export const recentLogs = (limit?: number) =>
  invoke<EventLog[]>("recent_logs", { limit });

export const listPlatforms = () => invoke<PlatformInfo[]>("list_platforms");

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

export const saveSmsConfig = (params: {
  access_key_id: string;
  access_key_secret: string;
  sign_name: string;
  template_code: string;
  phone_numbers: string;
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

export const createCustomPlatform = (params: {
  id: string;
  name: string;
  creator_url: string;
  publish_url: string;
  title_selector?: string;
  content_selector?: string;
  submit_selector?: string;
  logged_in_pattern?: string;
}) => invoke<void>("create_custom_platform", params);

export const deleteCustomPlatform = (id: string) =>
  invoke<void>("delete_custom_platform", { id });
