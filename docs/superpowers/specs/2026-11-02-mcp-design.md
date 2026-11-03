# 营销通内置 MCP HTTP 服务设计

## 目标

在营销通 GUI 进程内提供仅本机可访问的 MCP HTTP 服务，让 Cursor、Claude Code 等支持 HTTP MCP 的客户端查询账号、检测会话和调用发布能力。服务默认关闭；写入能力默认关闭，由设置页控制。

## 架构

Rust 侧新增 `src-tauri/src/mcp/` 模块：

- `config.rs`：配置模型，复用 SQLite `settings` 表的 `mcp_config` 键。
- `protocol.rs`：JSON-RPC 2.0 和 MCP 握手/错误结构。
- `tools.rs`：工具定义、参数 schema、权限拦截和工具分发。
- `server.rs`：axum 本地 HTTP 服务，监听 `127.0.0.1`。
- `mod.rs`：MCP 状态、启动/停止生命周期和 Tauri 配置命令。

MCP 工具通过 `AppHandle::state` 取得 Db、SessionRegistry 和 ObscuraManager，复用现有 `accounts.rs` 命令函数，避免重新实现登录、会话和发布逻辑。

## HTTP 协议

服务地址默认为 `http://127.0.0.1:8931/mcp`，只接受 POST JSON-RPC 请求。支持 `initialize`、`notifications/initialized`、`ping`、`tools/list` 和 `tools/call`。解析失败、方法不存在和参数错误返回标准 JSON-RPC 错误码；工具执行失败返回 MCP `isError: true` 内容。

令牌非空时要求 `Authorization: Bearer <token>`。服务不绑定外网地址，不提供跨域头。

## 配置

- `enabled`：是否启动服务，默认 `false`。
- `port`：监听端口，默认 `8931`。
- `token`：可选访问令牌。
- `allow_write`：是否允许发布、建号、删号、开关引擎和打开会话，默认 `false`。

保存配置时先停止旧服务，再按新配置重新启动；端口占用直接返回错误。应用启动时读取配置，若启用则自动启动。

## 工具

只读工具：`list_accounts`、`list_platforms`、`engine_status`、`check_account_session`、`recent_logs`、`check_sensitive_words`。

写工具：`open_account_session`、`batch_publish`、`create_account`、`delete_account`、`start_engine`、`stop_engine`。写权限关闭时，工具仍出现在列表中但调用返回错误内容，便于客户端提示用户打开设置。

## 前端

新增 `McpSettings.vue`，放入设置弹窗。提供服务开关、端口、令牌、写入开关、运行状态、连接地址、测试连接和 JSON 客户端配置复制。

## 验证

不做本地构建。GitHub Actions 的 `pnpm build` 和三平台 Tauri 打包作为编译门禁；发布前通过设置页「测试连接」验证 MCP 端到端响应。
