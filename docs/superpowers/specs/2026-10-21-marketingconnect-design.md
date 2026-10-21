# 营销通 (MarketingConnect) 设计文档

日期：2026-10-21
状态：已确认，进入实现

## 1. 项目概述

营销通是多平台矩阵营销号统一管理与发布工具。核心能力：多平台兼容、多帐号登录态隔离、选项卡式切换、代理支持、自定义平台、定时自动发布、帐密加密存储与自动续期、掉号提示、短信提醒登录。

## 2. 技术栈

| 层 | 选型 | 理由 |
|---|---|---|
| 桌面外壳 | Tauri 2 (Rust) | 体积小、性能好、Rust 生态 |
| 前端 | Vue 3 + TypeScript + Vite | 选项卡式工作台 |
| 浏览器引擎 | Obscura (Rust, Apache 2.0) | ~70MB/30MB 内存，内置 stealth 反指纹，CDP 兼容 |
| CDP 驱动 | chromiumoxide (纯 Rust) | 与 Tauri 同生态，不引入 Node 运行时 |
| 存储 | SQLite (rusqlite) | 账号/代理/任务/日志/cookie |
| 调度 | tokio-cron-scheduler | 定时发布任务 |
| 加密 | AES-GCM + 机器绑定密钥 | 帐密与 cookie 加密存储 |
| 通知 | 系统托盘 + 短信网关 API | 掉号提醒 |

## 3. 架构

```
┌──────────────────────────────────────────────┐
│  Tauri 2 外壳 (Rust)                          │
│  ├─ Vue 3 前端（选项卡式账号工作台）             │
│  ├─ SQLite（账号/代理/任务/日志/cookie 存储）    │
│  ├─ 调度器（tokio-cron-scheduler）             │
│  ├─ 凭据加密（AES-GCM + 机器绑定密钥）           │
│  └─ 通知（系统托盘 + 短信网关）                  │
├──────────────────────────────────────────────┤
│  Obscura Sidecar（~70MB，Tauri externalBin）   │
│  ├─ obscura serve --port 9222 --stealth       │
│  ├─ 每账号一个 BrowserContext（cookie 隔离）     │
│  ├─ 每会话指纹随机化 + 3520 个追踪域名拦截        │
│  └─ CDP WebSocket 接口                         │
├──────────────────────────────────────────────┤
│  CDP 驱动层（chromiumoxide，纯 Rust）           │
│  └─ 平台适配器（登录/发布/会话检测）              │
└──────────────────────────────────────────────┘
```

## 4. 核心机制

### 4.1 多账号隔离
每个账号登录时通过 CDP `Target.createBrowserContext` 创建独立上下文：cookie/storage 完全隔离，且 Obscura stealth 模式为每个会话自动随机化指纹（GPU/屏幕/canvas/音频/电池、userAgentData、webdriver=undefined、native function masking）。账号 ↔ context 一对一映射，前端选项卡切换即切换对应 context 的截图/ screencast 画面。

### 4.2 会话持久化与掉号检测
- cookie 通过 CDP `Storage.getCookies` 导出 → AES-GCM 加密 → SQLite
- 应用重启后 `Storage.setCookies` 恢复会话
- 定时健康检查：导航到需登录页面，检测是否被重定向至登录页 → 掉号事件 → 托盘通知 + 短信
- 自动续期：掉号后用存储帐密自动重登（短信验证码场景转人工介入）

### 4.3 代理
Obscura `--proxy` 为实例级参数。每账号独立代理方案：按代理分组运行多个 `obscura serve` 实例（每实例 ~30MB 内存），账号绑定到对应实例的 context。

### 4.4 平台适配器
每个平台一个适配器模块，实现三个接口：`login`（登录流程）、`publish`（发布流程）、`check_session`（会话检测）。自定义平台 = 按接口规范新增适配器。

## 5. 风险与验证计划

1. **渲染兼容性（最大风险）**：Obscura 为 2026-04 新项目，原生渲染引擎与 Chromium 存在长尾差异。抖音/小红书创作者平台为超重 SPA，必须首先验证。
2. **脚本执行预算**：默认 30s（`OBSCURA_SCRIPT_DEADLINE_MS` 可调），重 SPA 需调高。
3. **cookie 跨重启持久化**：context 生命周期与 serve 实例绑定，需验证「导出-恢复」闭环。
4. **项目成熟度**：生产使用前观望版本演进，保留回退 Playwright+Chromium 的方案空间。

### Spike 验证清单（按优先级）
1. Obscura 正常加载并操作抖音创作者平台（渲染 + 登录 + 发布图文）
2. 多 BrowserContext 并发隔离稳定性（5 context 不串 cookie）
3. 指纹随机化效果（各 context 指纹差异检测）
4. cookie 导出/恢复闭环（重启后会话保持）
5. 掉号检测准确性

## 6. 实现顺序

1. 脚手架：Tauri 2 + Vue 3 + TS + Obscura sidecar 启动链路
2. Spike 第 1 项：抖音平台兼容性（决定引擎选型）
3. 核心模块：账号管理 + SQLite + CDP 驱动封装
4. 前端：选项卡式账号工作台
5. 调度 + 通知 + 加密
6. 平台适配器：抖音优先
