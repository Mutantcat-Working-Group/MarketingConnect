<div align=center>
<img src="icon.png" style="width:100px;" width="100"/>
<h2>营销通</h2>
</div>

### 一、产品概述

- 多平台矩阵营销号统一管理与发布工具，支持 macOS / Windows / Linux 三平台。
- 一个账号一个独立浏览器环境（Cookie 隔离），登录态互不干扰，选项卡随时切换。
- 每账号可配独立代理，按代理自动分组运行多个浏览器引擎实例。
- 帐密与 Cookie 加密存储（AES-256-GCM + 系统钥匙串），重启自动恢复登录态。
- 掉号主动告警：后台定时检测 + 系统通知 + 短信双通道提醒。

核心价值：

- 多平台、多账号集中管理，矩阵营销分发一站完成。
- 引擎使用真实 Chrome，以普通 UA 与关闭自动化特征标记运行，并按代理隔离出口，降低矩阵账号关联风险。
- 支持自定义平台，不受内置平台列表限制。
- 数据全部本地加密存储，不经过第三方服务器。

### 二、功能说明

#### 账号管理

- 多账号登录态隔离（独立 BrowserContext），选项卡式工作台随时切换。
- 建档只需平台与备注名；用户名、密码可选。右键账号选择「编辑账号」修改资料、代理或分组，密码留空保留，支持明确清除。
- 密码、Cookie 与当前平台 origin 的 localStorage/sessionStorage 加密保存；下次打开恢复。平台要求重新验证时仍需验证码或扫码，不保证永久免登录。
- 登录是**交互式**的：打开登录会话后，在账号页面的截图视图里点击、输入、按键、滚动、扫码或输密码（前端 `login_action` + `finish_account_login`），确认登录后凭据加密持久化。`PlatformAdapter::login`（自动化账密登录）目前没有任何调用方，保留给未来的自动重登。
- 登录页面支持点击、输入、按键和滚动；可使用平台原生密码、短信验证码、扫码流程（具体可用方式由平台决定）。
- 账号分组管理，侧边栏按分组展示与计数。
- 登录状态必须有正向认证证据，不再仅凭地址没有 login 判定在线。平台结构变化可能导致无法自动确认，自定义平台应配置明确登录成功 URL 特征。

#### 代理支持

- 每账号独立代理地址，按代理分组自动启动多个引擎实例。
- 每个实例使用独立的 Chrome profile 目录（应用数据目录下 `chrome-profile/<键>`），代理出口随之隔离。

#### 发布能力

- 单账号发布：标题 / 正文 / 媒体路径，支持 cron 定时发布。
- 批量发布：同一内容一次分发到多个账号，逐账号结果反馈。
- 敏感词检测：广告法极限词、医疗、金融违规词库，发布前检查。

#### 平台适配

内置平台共 6 个，另可注册「自定义平台」。**自动发布是否已实现按平台区分，下表如实标注**；`publish=false` 表示该平台的 `publish()` 目前只会返回错误。

| 平台 | 自动发布 | 支持的发布字段 | 登录方式 |
|---|---|---|---|
| 抖音 | ❌ 未实现 | — | 交互式登录（会话持久化） |
| 微信公众号 | ❌ 未实现 | — | 交互式登录（会话持久化） |
| 小红书 | ✅ | 仅文本（标题 / 正文）；不读媒体 / 封面 / 话题 | 交互式登录（会话持久化） |
| CSDN | ✅ | 仅文本（Markdown 标题 / 正文）；不读媒体 / 封面 / 话题 | 交互式登录（会话持久化） |
| 知乎 | ✅ | 文本 + 图片（含多图）+ 话题；**不支持封面** | 交互式登录（会话持久化） |
| Dev.to | ✅ | 文本 + 标签（最多 4 个，仅字母数字）；**不支持封面** | **令牌型**：粘贴 API 令牌 |
| 自定义平台 | ✅ | 按配置的选择器发布文本 | 交互式登录（会话持久化） |

- 抖音、微信公众号的自动发布**尚未实现**：抖音需交互验证发布页选择器，公众号正文为 iframe 富文本编辑器。发布前校验会直接拒绝并提示改用「自定义平台」配置选择器，或打开会话后手动发布。
- 知乎 / 小红书 / CSDN 的发布页选择器在代码里是多候选列表，失败时报错会列出全部尝试过的选择器；本批次**未对真实平台做过端到端发布验证**（详见「已知限制」与 `docs/superpowers/plans/2026-11-06-batch1-verification.md`）。
- Dev.to 是令牌型平台：发布走 `POST https://dev.to/api/articles`，发布前先用 `GET /api/users/me` 校验令牌，全程不打开浏览器会话。设置封面需要先上传取得托管 URL，本批次未实现。
- 自定义平台：注册任意平台的创作者页 / 发布页 URL 与表单选择器，按配置发布文本。

#### MCP 接入

- 在右上角「设置」→「MCP 接入」启用，默认地址为 `http://127.0.0.1:8931/mcp`。
- 支持 HTTP MCP 客户端查询账号、平台、日志、引擎状态和敏感词；开启「允许写入」后可批量发布、建号、删号、控制引擎。
- 可设置 Bearer 令牌，复制客户端 JSON 配置；仅监听本机，应用退出后服务不可用。
- 默认关闭服务和写入权限；「测试连接」可检查服务与工具列表。

#### 掉号提醒

- 后台每 5 分钟自动检测已登录会话，掉号即告警。
- 系统通知 + 阿里云短信（配置后）双通道提醒。
- 关闭窗口最小化到托盘，后台监控不中断。

#### 定时发布行为

- 填 cron 表达式即注册定时作业，留空则立即发布；两者都受「登录编辑中」保护，不会抢占登录页面。
- 应用启动时会按数据库记录重建全部 cron 任务，重启不会丢失定时计划。
- cron 表达式无效时直接报错并把任务标记为失败，不会出现「创建成功但永远不执行」。
- 应用完全退出期间不会发布，下次启动后按 cron 继续。

### 已知限制

- 登录状态检测依赖平台页面上的正向证据（退出登录入口、账号标识或自定义平台的登录成功 URL 特征）。平台改版后可能无法自动确认；此时可在确认已进入平台后台后点「页面已登录，仍要保存」（仍要求当前页面在平台域名下）。
- 凭证保存使用页面级 `page.get_cookies()`，**受当前页面 URL 过滤**，只覆盖当前页面域名的 Cookie；跨域 SSO 的其他域名 Cookie 可能未纳入。localStorage/sessionStorage 只保存并恢复到平台的 creator origin（创作者页来源），其他 origin 的 Web Storage 不参与。
- 平台要求短信验证码或扫码重新验证时，仍需要人工完成，无法永久免登录。
- MCP 访问令牌**明文保存在本地 SQLite 配置行中**（未额外加密），服务仅监听 127.0.0.1。
- CDP 中继（`src-tauri/src/obscura/relay.rs`）负责弥合浏览器引擎与驱动库（chromiumoxide 0.7）之间的事件字段差异——一条事件解析失败就会让整条 CDP 连接断开。中继做三类处理：字段缺失时补上引擎漏掉的必填字段（`Network.requestWillBeSent` 的 `initialPriority` / `referrerPolicy` / `redirectHasExtraInfo`，`Network.responseReceived` 的 `charset` / `connectionReused` / `connectionId` / `encodedDataLength` / `securityState` / `hasExtraInfo`），字段齐全时原样透传；规范化新版 Chromium 与驱动库的版本差事件（`Network.requestWillBeSentExtraInfo` 的新 `clientSecurityState`、`Page.frameRequestedNavigation` 的未知 `reason`，如 `initialFrameNavigation`）；若修补后仍无法被驱动库解析，则**丢弃该事件并告警**（源码日志文案为 `丢弃无法解析的 CDP 事件（方法 {}）：继续转发会断开整条连接`），而不是让整条连接断开——响应帧（带 `id`）永不丢弃。排查同类问题可用 `cargo run --example cdp_patch_probe -- <消息转储>` 重新探测缺哪些字段，转储通过环境变量 `MC_CDP_DUMP=<路径>` 打开（文件含 CDP 原始流量，排查后请删除）。
- **字体不加载的问题只在把引擎显式指向 Obscura（`MC_ENGINE_CMD`）时才会出现**：Obscura 默认不加载任何字体，不指定字体目录时页面所有文字都会渲染成方块（中文尤其明显）；且其**表单控件（`<input>`/`<select>`）文字绘制不走加载的字体**，内容与占位提示仍会显示为方块（`textarea`、`contenteditable` 正常），这是引擎自身缺陷，应用侧无法修。应用会按平台传入系统字体目录（macOS `/System/Library/Fonts`、`/Library/Fonts`、`~/Library/Fonts`；Windows `%SystemRoot%\Fonts` 等；Linux `/usr/share/fonts` 等），可用环境变量 `MC_FONT_DIRS`（`:` 或 `;` 分隔）追加自定义目录。**默认真 Chrome 原生加载系统字体，没有这个问题。**
- **默认引擎是真实 Chrome，不是 Obscura。** 解析顺序为：`MC_ENGINE_CMD`（显式覆盖）→ 本机安装的 Chrome → 应用数据目录中已下载的 Chrome for Testing → 自动下载。自动下载锁定版本 `157.0.8103.38`（常量 `LOCKED_CFT_VERSION`），下载地址取自 Chrome for Testing 官方清单；引擎安装在应用数据目录下 `engine/<版本>/chrome-<平台键>/`，日志会打印 `使用引擎: <路径>（来源: System / Downloaded / Explicit）`。`MC_ENGINE_CMD` 是**显式覆盖入口**（调试 / 高级用户，也可用于指向 Obscura 做对照），模板支持 `{port}`、`{proxy}` 占位符，含空格的路径用引号，例如：
  `MC_ENGINE_CMD='"/path/to/Google Chrome for Testing" --headless=new --remote-debugging-port={port} --user-data-dir=/tmp/mc-chrome'`
  之所以默认改用真 Chrome：Obscura 无法完成带风控平台的登录。实测抖音登录页的 `/passport/web/challenge/` 在 Obscura 里被服务端判为「参数错误」，登录 SDK 随即中止二维码状态轮询（真 Chromium 90 秒轮询 76 次，Obscura 仅 2 次），手机端显示登录成功而网页端永远收不到；微信、小红书等使用同类风控。
- 引擎多实例是**按代理**分组的，每个实例使用独立 Chrome profile 目录。同一 `--user-data-dir` 的第二个 Chrome 进程会因 `ProcessSingleton` 冲突**直接中止**，因此多代理场景必须分目录。
- **本批次未对任何真实平台做过端到端发布验证**：CI 只证明编译、打包与单元测试通过。知乎 / 小红书 / CSDN 的发布页选择器是**未实测的候选**；抖音 / 公众号的自动发布尚未实现；Dev.to 的令牌发布流程虽已实现，也未用真实令牌验证过。详见 `docs/superpowers/plans/2026-11-06-batch1-verification.md`。
- `PlatformAdapter::login`（自动化账密登录）目前**没有任何调用方**，保留给未来的自动重登；实际登录是交互式的（前端 `login_action` + `finish_account_login`）。
- 单元测试 `crypto::tests::test_encrypt_decrypt_roundtrip` 依赖系统钥匙串，在无人值守环境会挂起；CI 与本地验证均用 `--skip test_encrypt_decrypt_roundtrip` 跳过。
- 页面脚本自行跳转的瞬间，引擎会重建执行上下文，可能出现一次「上下文失效」错误；页面求值已内置自动重试（`src-tauri/src/obscura/eval.rs`），只对该类错误重试，不掩盖真实脚本异常。
- 登录页的「还在显示登录 UI」判定最早只识别语义化控件（`a`/`button`/`[role=button]`），而抖音、小红书的登录控件是普通 `div`，导致该保护失效；现已把「文本精确匹配 + `cursor: pointer`」的元素也算作控件。

### 三、安装与下载

从 [Releases](https://github.com/Mutantcat-Working-Group/MarketingConnect/releases) 下载对应平台安装包（每个平台提供两种架构，文件名里带架构后缀）：

- macOS Apple Silicon（M 系列）：`MarketingConnect_1.0.20261115_aarch64.dmg`
- macOS Intel（x64）：`MarketingConnect_1.0.20261115_x64.dmg`
- Windows x64：`MarketingConnect_1.0.20261115_x64-setup.exe`
- Linux x64：`MarketingConnect_1.0.20261115_amd64.AppImage`
- Linux arm64：`MarketingConnect_1.0.20261115_aarch64.AppImage`

> Windows 目前**只提供 x64 包**，暂不提供 Windows arm64 包。Windows on ARM 上可运行 x64 包（系统自带 x64 兼容层）。

> 安装后的软件名为中文「**营销通**」（macOS 的 `营销通.app`、Windows 的安装目录与开始菜单快捷方式、Linux 的桌面项）。
> 下载文件名用 ASCII 的 `MarketingConnect`，是因为 GitHub Releases 会自动剥离资源名中的非 ASCII 字符。

源码自编译需 Node.js 20+、pnpm 9+、Rust 1.75+，Linux 需 `libwebkit2gtk-4.1-dev` 等 Tauri 系统依赖；推送到 main 或打 v* 标签时 GitHub Actions 自动构建各平台各架构安装包（macOS x86_64/aarch64、Linux x86_64/aarch64 交叉或原生编译，Windows x64）。

真 Chrome 是默认引擎，无需预先安装：本机没有 Chrome 时，首次启动会自动下载对应平台的 Chrome for Testing（约 187 MB），之后复用。CI 构建时仍会执行 `./scripts/download-obscura.sh` 把 Obscura 二进制作为 sidecar 打进安装包（`externalBin`），它只在用 `MC_ENGINE_CMD` 显式指定时才被使用；交叉编译时用 `OBSCURA_TARGET=<目标三元组> ./scripts/download-obscura.sh` 指定该二进制的架构。

### 四、快速上手

1. 启动引擎：顶栏点击「启动引擎」。首次无本机 Chrome 时会自动下载 Chrome for Testing，稍等片刻。
2. 新增账号：左侧「新增账号」，选择平台、填写备注；登录资料、代理和分组可选。已有账号右键编辑。
3. 打开会话：账号工作台点击「打开登录会话」。
4. 交互式登录：点击截图内的平台控件选择密码/短信/扫码；点击目标输入框，在下方输入内容后「发送并清空」。验证码不保存；二维码用手机扫码。登录方式标签只显示说明，不会替平台切换模式。Dev.to 这类令牌型平台则在账号页面粘贴 API 令牌，保存时立即校验。
5. 保存凭证：默认开启「自动确认登录并保存」，检测到登录成功后加密持久化；也可手动点击「确认登录并保存会话」。只有平台认证证据与凭证写入都成功才显示已登录。可开启自动刷新画面，刷新不会跳转。登录编辑期间后台巡检和发布暂停。
6. 创建发布任务：填写内容（可先「检测敏感词」）。填 cron 表达式会定时执行，留空则立即发布。填写超出该平台能力的字段（如给小红书加图片）会在发布前直接报错；抖音、公众号的自动发布尚未实现，会提示改用手动发布或自定义平台。
7. 批量发布：顶栏「批量发布」，勾选多个账号一次分发同一内容，会按各平台能力分别校验，逐账号返回结果。
8. 掉号提醒：后台自动检测，掉号时系统通知 + 短信提醒。
9. 关闭窗口即最小化到托盘，托盘左键单击恢复窗口。

### 五、技术栈

- 桌面外壳：Tauri 2（Rust）
- 前端：Vue 3 + TypeScript + Vite
- 浏览器引擎：**Chrome / Chrome for Testing**（本机 Chrome 优先，缺失时自动下载 Chrome for Testing，版本由配置锁定 `157.0.8103.38`）；仅在用 `MC_ENGINE_CMD` 显式指定时才使用 [Obscura](https://github.com/h4ckf0r0day/obscura)（Rust 无头浏览器）
- CDP 驱动：chromiumoxide（中间隔一层本机 CDP 中继，用于补齐引擎缺失的事件字段，见「已知限制」）
- 存储：SQLite + AES-256-GCM 加密
- 调度：tokio-cron-scheduler
- 短信：阿里云短信服务

### 六、开发进度

- [X] 多平台适配器 6 个（抖音 / 小红书 / 公众号 / CSDN / 知乎 / Dev.to）+ 自定义平台注册
- [X] 真实 Chrome 引擎（本机优先 + 自动下载 Chrome for Testing + 版本锁定）
- [X] 诚实的能力矩阵（`publish` / 文本 / 图片 / 多图 / 封面 / 话题 / 线程 / 平台内定时），发布前按平台校验
- [X] 小红书自动发文（仅文本：标题 / 正文 / 发布；选择器未实测）
- [X] CSDN 自动发文（仅文本：Markdown 标题 / 正文 / 发布；选择器未实测）
- [X] 知乎适配器（文本 + 图片含多图 + 话题，不支持封面；发布选择器未实测）
- [X] Dev.to 适配器（令牌型，API 发布文本 + 标签；未用真实令牌验证）
- [X] 多账号登录态隔离
- [X] 选项卡式账号工作台
- [X] 代理地址支持（每代理独立引擎实例与独立 Chrome profile）
- [X] 自定义平台（URL + 表单选择器）
- [X] 自动化定时发布（cron）
- [X] 帐密加密存储与自动续期
- [X] 掉号提示（后台监控 + 系统通知 + 短信）
- [X] 短信提醒（阿里云）
- [X] 页面截图（无头登录辅助）
- [X] 账号分组
- [X] 批量发布
- [X] 敏感词检测
- [X] 系统托盘
- [X] 打开登录会话稳定可用（CDP 中继修补引擎缺失事件字段 + 求值自动重试）
- [ ] 抖音自动发布流程（当前走自定义平台选择器方案）
- [ ] 公众号自动发文（正文为 iframe 编辑器，需交互验证后实现）
- [ ] 真实平台端到端发布验证（需要真实账号与令牌，本批次未做）
- [ ] Dev.to 封面（需先上传取得托管 URL）

### License

GPL-3.0 © Mutantcat，详见 [LICENSE](LICENSE)。
