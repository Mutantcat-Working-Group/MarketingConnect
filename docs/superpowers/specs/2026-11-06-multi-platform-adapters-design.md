# 多平台适配器扩展设计

## 用户需求

在现有 4 个平台（抖音、小红书、公众号、CSDN）之外，新增约 24 个国内外平台，发布能力做到完整：文本、多图、视频、封面、话题标签、线程、平台内定时发布。

## 调研结论（2026-11-06）

同类开源项目的路线分歧非常明确：

| 项目 | 星数 | 路线 | 对本项目的意义 |
|---|---|---|---|
| `dreammis/social-auto-upload` | 16170 | Playwright + cookie，本地会话别名 | 国内主力方案。**对 Facebook/Instagram/YouTube 也故意用浏览器自动化而非官方 API**，其 README 明示原因是官方 API 有审核与配额限制 |
| `gitroomhq/postiz-app` | 40978 | 官方 API + OAuth，31 个 provider | 它是 SaaS，可以用自有注册应用；桌面端无法照搬 |
| `inovector/mixpost` | 3911 | 官方 API | 同上 |
| `hanliang97/MatrixMedia` | 1224 | Vue + Electron + Puppeteer | 与本站形态最接近（桌面端矩阵发布） |

两个关键结论：

1. **桌面端不能照搬 OAuth 路线。** 注册 Meta / X / LinkedIn / Pinterest 开发者应用需要过审，且桌面二进制内嵌 client secret 不可行。
2. **模型一致性已验证。** social-auto-upload 采用「每个平台先登录一次 → 本地会话别名区分多账号 → 之后检查会话」，与本站现有的账号 + cookie 会话模型一致，无需推翻。

因此确定路线：**浏览器自动化为主干 + 少量「自助令牌、无需审核」的 API 适配器**。

## 当前代码确认的问题

1. **完全没有文件上传实现**：全仓库无 `set_input_files` / `DOM.setFileInputFiles` 调用。所有图片、视频平台（含已有的小红书）在发布环节都无法带媒体。
2. **`media_paths` 数据流是断的**：`db` → `scheduler` → `PublishContent` 一路贯通，但**没有任何适配器消费它**，字段到达适配器即被丢弃。
3. `PublishContent` 只有 `title` / `text` / `media_paths`，缺少封面、话题、线程、定时字段。
4. `PlatformAdapter` 没有能力声明，前端无法在填写阶段就禁用不支持的能力，只能等发布时报错。
5. 多候选选择器工具 `fill_first` / `click_first` 只在 `mod.rs`、`xiaohongshu.rs`、`csdn.rs` 使用；`douyin.rs`、`wechat_mp.rs` 仍使用单点选择器。
6. 令牌型平台没有登录入口：现有交互只有扫码/短信，无法粘贴 API 令牌。
7. 发布页选择器从未对真实平台验证（README 已记录的已知限制）。

## 推荐实现

### 一、架构：两种适配器族 + 共享内核

扩展 `PlatformAdapter`：

```
PlatformAdapter
├─ capabilities() -> Capabilities   新增
├─ login() / check_session()        已有
└─ publish()                        语义扩展为完整发布
```

- **族 A `BrowserAdapter`**：国内全部 11 个，以及国外 X / Facebook / LinkedIn / Pinterest / Threads / Medium / Substack / Tumblr。
- **族 B `TokenAdapter`**：Dev.to / Hashnode / WordPress / Mastodon / Reddit，走自助令牌 API，无需浏览器与选择器。

共享内核是 24 个平台可维护的前提，否则就是 24 份样板：

| 模块 | 职责 |
|---|---|
| `select` | 多候选选择器：`fill_any` / `click_any` / `wait_any`，把现有 `fill_first` / `click_first` 补全为统一工具 |
| `media` | `set_input_files` + 拖拽兜底 + 上传完成等待（新增） |
| `thread` | 顺序发帖，支持 X / Threads / Mastodon 的线程 |
| `schedule` | 平台内定时发布 |
| `probe` | 复用现有 `authenticated_probe` |

### 二、能力矩阵

每个适配器声明能力集合：`文本 / 图片 / 视频 / 多图 / 封面 / 话题 / 线程 / 平台内定时`。

前端据此显示或禁用输入项并给出明确提示；后端发布前二次校验，避免运行到一半才失败。

### 三、数据模型

- `PublishContent` 增加 `cover_path`、`topics`、`thread_segments`、`schedule_at`。
- 发布任务表增加对应列，保证老数据可迁移（新增列可空，旧行不受影响）。

### 四、登录

- 浏览器族：**完全复用**现有「打开会话 → 扫码/短信 → 确认保存」与 cookie 导出流程，不需新写。
- 令牌族：新增「粘贴令牌」入口，校验通过后立即生效，不走浏览器。

### 五、选择器验证策略

必须区分两类，无法一视同仁：

- **登录页选择器：可在实现阶段用真 Chromium 逐个实测**（登录页公开），结论写入实现记录。
- **发布页选择器：无法验证**（在登录态之后）。只用多候选列表 + 明确报错，并提供「选择器自检」诊断命令，让用户登录后能自行定位该改哪个选择器。

## 平台清单与分批

### 批次 1：地基

能力矩阵 + 媒体上传内核 + 选择器内核 + 各打样 1 个平台：**知乎**（浏览器族，代表批次 2 的结构）与 **Dev.to**（令牌族，结构最简单、无需浏览器）。此批次不追平台数量，只验证地基可用。

### 批次 2：国内内容社区（8）

| 平台 | 入口 | 能力 |
|---|---|---|
| 知乎 | zhihu.com | 文本、多图、封面、话题 |
| B站 | member.bilibili.com | 视频、封面、话题 |
| 微博 | weibo.com | 文本、多图、话题 |
| 头条号 | mp.toutiao.com | 文本、多图、封面 |
| 百家号 | baijiahao.baidu.com | 文本、多图、封面 |
| 掘金 | juejin.cn | 文本、封面、话题 |
| 简书 | jianshu.com | 文本、多图 |
| 博客园 | cnblogs.com | 文本、封面 |

### 批次 3：令牌族 + 国外博客（8）

| 平台 | 登录方式 | 能力 |
|---|---|---|
| Dev.to | API key | 文本、封面、话题 |
| Hashnode | 令牌 | 文本、封面、话题 |
| WordPress | 应用密码 | 文本、多图、封面、话题、定时 |
| Mastodon | 实例令牌 | 文本、多图、话题、线程、定时 |
| Reddit | script 应用 | 文本、多图 |
| Medium | 令牌或浏览器 | 文本、封面、话题 |
| Substack | 仅浏览器 | 文本、封面 |
| Tumblr | 浏览器 | 文本、多图、话题、定时 |

### 批次 4：国外社交（5）

X、Facebook、LinkedIn、Pinterest、Threads。X / Threads 支持线程与定时；Pinterest 本质是图片平台。

### 批次 5：国内视频（3）

快手、视频号、西瓜视频，均为视频 + 封面 + 定时。

## 前置阻塞项：引擎方案（已定）

**已确认：双轨制。** 优先使用本机已安装的 Chrome，缺失时自动下载 Chrome for Testing 到应用数据目录。国内 11 个平台全依赖真 Chromium 登录（Obscura 无法通过 ByteDance 系风控，实测登录 SDK 会中止二维码轮询），引擎决策在顺序上早于适配器实现。

引擎解析链：

1. 环境变量 `MC_ENGINE_CMD`（显式覆盖，调试与高级用户用）
2. 本机 Chrome（macOS / Windows / Linux 的常见安装路径）
3. 应用数据目录下已下载的 Chrome for Testing
4. 都没有则自动下载

下载实现要点：

- 版本来源 `https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json`（已验证可达），按平台取 `mac-arm64` / `mac-x64` / `win64` / `linux64` 的 zip。
- **必须锁定具体版本号写入配置，不要在每次启动时取最新。** 实测该接口当前 stable 为 `157.0.8103.38`，而本次中继补丁是用 `151.0.7922.34` 的流量推导的；版本漂移会引入新的未知事件。
- 下载由应用自身发起（非浏览器下载），不经过 LaunchServices，因此不会被打上 `com.apple.quarantine` 隔离属性；即便如此仍应在启动失败时给出可操作的提示，而不是静默失败。
- 解压后校验可执行文件存在且能启动，再写入配置记为该版本可用。

## 中继健壮性：未知事件兜底

引擎版本漂移会持续产生新的 CDP 事件形状，逐个打补丁是打地鼠。因此中继除现有已知补丁外，增加一层兜底：

**对每个事件用 chromiumoxide 的反序列化器试解析，解析失败的事件直接丢弃并记录警告，而不是让整条连接断开。**

理由：解析失败的事件本来就不会被消费（继续转发只会杀掉连接），丢弃它是严格更优的选择；同时把「又一个平台打不开」降级为「一条可排查的日志」。实现需要把 `chromiumoxide_cdp` / `chromiumoxide_types` 提升为直接依赖（已在 lock 文件中，可离线解析）。

## 验证策略

- 登录页选择器：真 Chromium 实测。
- 发布流程：多候选选择器 + 明确错误信息；不声称已对真实平台验证。
- 单元测试覆盖共享内核（选择器回退顺序、能力校验、数据模型迁移）。
- CI 只证明编译、打包与单元测试通过，**不能证明任何真实平台可发布**。

## 其他可选方案

- **预置平台库（配置驱动，不写代码）**：用现有 `CustomPlatformAdapter` 通道把平台做成预设数据。成本低、覆盖快，但无法实现多图、封面、话题、线程与平台内定时，达不到本次「完整能力」的目标。可作为批次交付前的过渡手段。
- **国外全走官方 API（对齐 Postiz）**：最稳定，但需要为每个平台注册开发者应用并过审，Meta 系基本无法完成，桌面端内嵌密钥也不可行。
- **可见 Chromium 窗口**：登录交互更直观、兼容面更大，但增加跨平台打包成本，且与「后台批量发布」的产品定位冲突。

## 已确认决策

1. **引擎**：双轨制——本机 Chrome 优先，缺失时自动下载 Chrome for Testing 并锁定版本（见「前置阻塞项」）。
2. **验证方式**：允许在本地真实 Chromium 中实测各平台登录页选择器，并向用户报告实测结果；打包与发布仍走 GitHub Actions。单元测试与前端构建本地执行属于常规开发验证，不视为替代 CI。
