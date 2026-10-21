# 营销通 (MarketingConnect)

多平台矩阵营销号统一管理与发布工具。

## 功能规划

- [x] 多平台兼容（平台适配器架构）
- [x] 多帐号登录态隔离（Obscura BrowserContext）
- [x] 选项卡式账号工作台
- [x] 代理地址支持（按代理分组实例）
- [ ] 自定义平台（适配器接口已就绪）
- [ ] 自动化定时发布（调度器骨架已就绪）
- [ ] 帐密加密存储与自动续期（加密层已就绪）
- [ ] 掉号提示（检测接口已就绪）
- [ ] 短信提醒登录

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面外壳 | Tauri 2 (Rust) |
| 前端 | Vue 3 + TypeScript + Vite |
| 浏览器引擎 | [Obscura](https://github.com/h4ckf0r0day/obscura)（Rust 无头浏览器，内置 stealth 反指纹） |
| CDP 驱动 | chromiumoxide（纯 Rust） |
| 存储 | SQLite（rusqlite） |
| 调度 | tokio-cron-scheduler |
| 加密 | AES-256-GCM + 系统钥匙串主密钥 |

## 架构

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

## 支持平台

| 平台 | 架构 | 状态 |
|---|---|---|
| macOS | Apple Silicon (arm64) / Intel (x86_64) | ✅ |
| Windows | x86_64 | ✅ |
| Linux | x86_64 / arm64 | ✅ |

## 开发环境要求

通用：
- Node.js 20+
- pnpm 9+
- Rust 1.75+

平台前置依赖：
- **macOS**：Xcode Command Line Tools（`xcode-select --install`）
- **Windows**：
  - Microsoft C++ 生成工具（Visual Studio Build Tools，Rust 编译需要）
  - WebView2 Runtime（Win10/11 通常已预装；[微软官方下载](https://developer.microsoft.com/microsoft-edge/webview2/)）
- **Linux**（Ubuntu/Debian）：
  ```bash
  sudo apt-get install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf build-essential
  ```
  > Obscura 的 Linux 二进制要求 glibc 2.35+（Ubuntu 22.04 及以上）

## 快速开始

```bash
# 1. 安装前端依赖
pnpm install

# 2. 下载 Obscura sidecar 二进制（按当前平台自动选择）
#    macOS / Linux:
./scripts/download-obscura.sh
#    Windows (PowerShell):
.\scripts\download-obscura.ps1

# 3. 启动开发模式
pnpm tauri dev
```

## 跨平台构建

推送到 `main` 或打 `v*` 标签时，GitHub Actions 会自动构建三个平台的安装包
（macOS dmg / Windows msi / Linux deb+AppImage），以 Draft Release 形式产出。

本地手动构建某个平台：
```bash
pnpm tauri build                          # 当前平台
pnpm tauri build --target aarch64-apple-darwin   # macOS Apple Silicon
```

## 项目结构

```
├── src/                        # Vue 3 前端
│   ├── api.ts                  # Tauri 命令封装（TypeScript 类型）
│   ├── App.vue                 # 主布局（顶栏 + 侧边栏 + 选项卡 + 工作台）
│   └── components/
│       ├── AccountSidebar.vue  # 账号管理（列表 + 新增表单）
│       ├── AccountWorkspace.vue# 发布工作台（任务创建 + 列表）
│       └── ...
├── src-tauri/                  # Rust 后端
│   ├── src/
│   │   ├── lib.rs              # 入口：模块注册 + Tauri 命令
│   │   ├── db.rs               # SQLite 数据层（账号/任务/日志）
│   │   ├── crypto.rs           # AES-GCM 加密（钥匙串主密钥）
│   │   ├── accounts.rs         # 账号管理 Tauri 命令
│   │   ├── scheduler.rs        # 定时发布调度器
│   │   ├── obscura/
│   │   │   ├── mod.rs          # sidecar 进程管理
│   │   │   └── client.rs       # CDP 客户端（chromiumoxide）
│   │   └── platforms/
│   │       ├── mod.rs          # 平台适配器 trait
│   │       └── douyin.rs       # 抖音适配器（骨架）
│   ├── binaries/               # Obscura sidecar（gitignore，脚本下载）
│   └── tauri.conf.json
├── scripts/
│   └── download-obscura.sh     # Obscura 二进制下载脚本
└── docs/superpowers/specs/     # 设计文档
```

## 当前进度

MVP 骨架已完成：Tauri 2 + Vue 3 工程、Obscura sidecar 集成、SQLite 数据层、
凭据加密、选项卡式 UI、平台适配器接口。

**下一步（Spike 验证）**：验证 Obscura 对抖音创作者平台的渲染兼容性，
通过后填充 `platforms/douyin.rs` 的登录与发布流程。

## License

私有项目 © Mutantcat
