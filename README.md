# 营销通 (MarketingConnect)

多平台矩阵营销号统一管理与发布工具。

## 功能规划

- [x] 多平台兼容（平台适配器架构 + 自定义平台注册）
- [x] 多帐号登录态隔离（Obscura BrowserContext，按代理分组多实例）
- [x] 选项卡式账号工作台
- [x] 代理地址支持（每代理组一个独立 obscura serve 实例）
- [x] 自定义平台（用户注册任意平台：URL + 表单选择器）
- [x] 自动化定时发布（cron 表达式注册调度器）
- [x] 帐密加密存储与自动续期（AES-GCM + cookie 持久化恢复）
- [x] 掉号提示（后台监控 + 系统通知 + 短信双通道）
- [x] 短信提醒登录（阿里云短信，配置于设置面板）
- [x] 页面截图（无头模式登录辅助：扫码/填表可视化）
- [x] 账号分组（自定义分组，侧边栏分组展示）
- [x] 批量发布（同一内容发布到多账号，逐账号结果反馈）
- [x] 敏感词检测（广告法极限词/医疗/金融违规词库，发布前检查）
- [x] 系统托盘（最小化到托盘，后台监控不中断）

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
│  ├─ Vue 3 前端（选项卡式账号工作台 + 设置面板）  │
│  ├─ SQLite（账号/代理/任务/日志/cookie/设置）    │
│  ├─ 调度器（tokio-cron-scheduler）             │
│  ├─ 凭据加密（AES-GCM + 机器绑定密钥）           │
│  └─ 通知（系统通知 + 阿里云短信）                │
├──────────────────────────────────────────────┤
│  Obscura Sidecar（~70MB，Tauri externalBin）   │
│  ├─ 多实例：每个代理组一个 serve 进程           │
│  │   （--proxy 为进程级参数，多实例实现每账号代理）│
│  ├─ 每账号一个 BrowserContext（cookie 隔离）     │
│  ├─ 每会话指纹随机化 + 3520 个追踪域名拦截        │
│  └─ CDP WebSocket 接口                         │
├──────────────────────────────────────────────┤
│  CDP 驱动层（chromiumoxide，纯 Rust）           │
│  └─ 平台适配器（登录/发布/会话检测）              │
│     ├─ 内置适配器（douyin）                     │
│     └─ 自定义平台适配器（DB 配置驱动）            │
└──────────────────────────────────────────────┘
```

## 使用流程

1. **启动引擎**：顶栏点击「启动引擎」（直连实例）
2. **新增账号**：左侧「新增账号」，选择平台（含自定义平台），填写帐密与代理，可填分组
3. **打开会话**：账号工作台点击「打开会话」——自动按代理启动对应实例
4. **手动登录**：查看页面截图，扫码或输入帐密完成登录
5. **保存 Cookie**：登录成功后点击「保存 Cookie」（加密持久化，重启自动恢复）
6. **创建发布任务**：填写内容（可先「检测敏感词」），可选 cron 表达式定时发布
7. **批量发布**：顶栏「批量发布」——勾选多个账号（支持按分组全选），一次分发同一内容
8. **掉号提醒**：后台监控每 5 分钟自动检测，掉号时系统通知 + 短信（如已配置）双通道提醒
9. **最小化到托盘**：关闭窗口不退出（后台监控继续），托盘左键恢复窗口

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
│   ├── App.vue                 # 主布局（顶栏 + 侧边栏 + 选项卡 + 工作台 + 设置弹层）
│   └── components/
│       ├── AccountSidebar.vue  # 账号管理（列表 + 新增表单）
│       ├── AccountWorkspace.vue# 发布工作台（会话 + 截图 + 任务）
│       ├── SmsSettings.vue     # 短信提醒配置
│       ├── CustomPlatforms.vue # 自定义平台管理
│       └── ...
├── src-tauri/                  # Rust 后端
│   ├── src/
│   │   ├── lib.rs              # 入口：模块注册 + Tauri 命令
│   │   ├── db.rs               # SQLite 数据层（账号/任务/日志/设置/自定义平台）
│   │   ├── crypto.rs           # AES-GCM 加密（钥匙串主密钥）
│   │   ├── sms.rs              # 阿里云短信（HMAC-SHA1 签名）
│   │   ├── accounts.rs         # 账号管理 Tauri 命令
│   │   ├── scheduler.rs        # 定时发布调度器
│   │   ├── session.rs          # 账号会话生命周期（context/cookie/截图）
│   │   ├── obscura/
│   │   │   ├── mod.rs          # sidecar 多实例管理（按代理分组）
│   │   │   └── client.rs       # CDP 客户端（chromiumoxide）
│   │   └── platforms/
│   │       ├── mod.rs          # 平台适配器 trait + 注册表
│   │       ├── douyin.rs       # 抖音适配器（骨架）
│   │       └── custom.rs       # 自定义平台适配器（配置驱动）
│   ├── binaries/               # Obscura sidecar（gitignore，脚本下载）
│   └── tauri.conf.json
├── scripts/
│   ├── download-obscura.sh     # Obscura 二进制下载脚本（macOS/Linux/Windows Git Bash）
│   └── download-obscura.ps1    # Windows PowerShell 版
└── docs/superpowers/specs/     # 设计文档（含 spike 验证记录）
```

## 当前进度

全部 9 项原始需求 + 竞品参考增强（账号分组、批量发布、敏感词检测）已实现，
三平台（macOS / Windows / Linux）CI 连续 7 次构建通过。

**Spike 验证结论**（详见设计文档）：Obscura 可完整渲染抖音创作者平台
（登录入口可见、JS 全量执行），「手动登录 + 加密 cookie 持久化」架构可行。

**下一步**：填充 `platforms/douyin.rs` 的自动发布流程
（当前为骨架，发布走自定义平台的选择器方案亦可覆盖）；
竞品的数据统计看板、评论私信聚合等重功能待后续版本。

## License

本项目采用 [GNU General Public License v3.0](LICENSE)（GPL-3.0）开源。

Copyright (C) 2026 Mutantcat

这意味着：
- ✅ 可自由使用、修改、分发本项目
- ✅ 修改后分发时必须同样以 GPL-3.0 开源并提供源码
- ❌ 不得将本项目（或其修改版）闭源集成到专有软件中分发

依赖组件协议：[Obscura](https://github.com/h4ckf0r0day/obscura)（Apache-2.0，与 GPL-3.0 兼容）、
Tauri（Apache-2.0 / MIT）。完整第三方协议见各依赖清单。
