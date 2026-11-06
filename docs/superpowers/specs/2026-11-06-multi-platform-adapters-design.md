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

## 前置阻塞项

**引擎方案必须先定。** 本批国内 11 个平台全部依赖真 Chromium 登录（Obscura 无法通过 ByteDance 系风控，实测登录 SDK 会中止二维码轮询）。若引擎仍是 Obscura，适配器写完也登录不上，因此引擎决策在顺序上早于适配器实现。

## 验证策略

- 登录页选择器：真 Chromium 实测。
- 发布流程：多候选选择器 + 明确错误信息；不声称已对真实平台验证。
- 单元测试覆盖共享内核（选择器回退顺序、能力校验、数据模型迁移）。
- CI 只证明编译、打包与单元测试通过，**不能证明任何真实平台可发布**。

## 其他可选方案

- **预置平台库（配置驱动，不写代码）**：用现有 `CustomPlatformAdapter` 通道把平台做成预设数据。成本低、覆盖快，但无法实现多图、封面、话题、线程与平台内定时，达不到本次「完整能力」的目标。可作为批次交付前的过渡手段。
- **国外全走官方 API（对齐 Postiz）**：最稳定，但需要为每个平台注册开发者应用并过审，Meta 系基本无法完成，桌面端内嵌密钥也不可行。
- **可见 Chromium 窗口**：登录交互更直观、兼容面更大，但增加跨平台打包成本，且与「后台批量发布」的产品定位冲突。

## 待确认项

1. 引擎方案：打包 Chromium（安装包增大）还是要求本机已安装 Chrome。
2. 本地验证方式：此前要求「完全用 GitHub Actions 打包与验证」，但选择器实测必须在本地真 Chromium 中进行，二者需要明确取舍。
