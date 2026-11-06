# Batch 1 验证记录（Task 12 实际执行）

> 对应计划：`docs/superpowers/plans/2026-11-06-multi-platform-batch1-foundation.md` 的 Task 12。
>
> **本文件只记录实际执行过的命令与真实输出。** 凡未执行的部分，一律在「未能验证」一节列出并说明原因；不得从「编译通过」推断「平台可发布」。

- 日期：2026-11-06 12:41 CST
- 分支：`feat/multi-platform-batch1`
- 代码基线：`615610a ci: 新增不含发布动作的功能分支验证 workflow`（本文件提交时，文档改动尚未提交）
- 宿主环境：macOS 27.2，arm64；`cargo 1.97.1 (c980f4866 2026-06-30)`，`rustc 1.97.1 (8bab26f4f 2026-07-14)`
- 前端工具链：`node v23.11.0`，`pnpm 12.15.0`，`vite v8.3.4`，`vue-tsc`（`pnpm run build` 内的 `--noEmit` 通过）
- 说明：当前 shell 的 `PATH` 不含 `~/.cargo/bin` 与 `pnpm`，因此命令前显式加上了工具路径。这不改变任何构建产物。

## 1. 已执行的验证

### 1.1 Rust 全量单元测试

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd src-tauri && cargo test --lib -- --skip test_encrypt_decrypt_roundtrip
```

结果：**通过**。原始输出关键行：

```
test result: ok. 99 passed; 0 failed; 1 ignored; 0 measured; 1 filtered out; finished in 0.01s
```

其中被 `--skip` 跳过的是 `crypto::tests::test_encrypt_decrypt_roundtrip`（依赖系统钥匙串，无人值守环境会挂起），`1 ignored` 是代码里本身标记为 `#[ignore]` 的用例。与本次改动相关的用例均通过，包括：

```
test obscura::relay::drops_event_that_patching_cannot_fix ... ok
test obscura::relay::never_drops_responses ... ok
test obscura::relay::unknown_reason_is_normalized_not_dropped ... ok
test obscura::relay::strips_client_security_state_from_157_shape ... ok
test obscura::engine::tests::user_agent_has_no_headless_marker ... ok
test platforms::capability_tests::unimplemented_publish_adapters_declare_no_capabilities ... ok
test platforms::capability_tests::validate_rejects_when_publish_is_unimplemented ... ok
test platforms::capability_tests::zhihu_does_not_declare_unimplemented_cover ... ok
test platforms::devto::tests::declares_api_capabilities ... ok
test platforms::devto::tests::publish_with_token_validates_before_any_network_call ... ok
```

### 1.2 Rust 编译

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd src-tauri && cargo build
```

结果：**退出码 0**，`Finished dev profile [unoptimized + debuginfo] target(s) in 8.33s`。为拿到完整警告列表，先 `touch src-tauri/src/lib.rs` 再编译，输出 **10 个 warning**：

```
warning: unused import: `csdn::CsdnAdapter`              --> src/platforms/mod.rs:27:9
warning: unused import: `CustomPlatformAdapter`          --> src/platforms/mod.rs:28:34
warning: unused import: `douyin::DouyinAdapter`          --> src/platforms/mod.rs:29:9
warning: unused import: `wechat_mp::WechatMpAdapter`     --> src/platforms/mod.rs:36:9
warning: unused import: `xiaohongshu::XiaohongshuAdapter`--> src/platforms/mod.rs:37:9
warning: method `get_account_password` is never used     --> src/db.rs:371:12
warning: associated function `internal` is never used    --> src/mcp/protocol.rs:78:12
warning: method `login` is never used                    --> src/platforms/mod.rs:157:14
warning: function `is_logged_in` is never used           --> src/platforms/wechat_mp.rs:39:4
warning: methods `len` and `is_empty` are never used     --> src/session.rs:276:12
warning: `marketing-connect` (lib) generated 10 warnings (run `cargo fix --lib -p marketing-connect` to apply 5 suggestions)
```

**无 error。** 其中 `workflow: method 'login' is never used --> src/platforms/mod.rs:157:14` 恰好印证了本文档与 README 的说法：`PlatformAdapter::login` 目前没有任何调用方。

### 1.3 前端构建（类型检查 + 打包）

```bash
export PATH="/opt/homebrew/bin:$HOME/.nvm/versions/node/v22.23.1/bin:$PATH"
node --version        # v23.11.0
pnpm --version        # 12.15.0
pnpm run build        # vue-tsc --noEmit && vite build
```

结果：**退出码 0**。原始输出：

```
$ vue-tsc --noEmit && vite build
vite v8.3.4 building client environment for production...
transforming...
✓ 41 modules transformed.
dist/index.html                   0.47 kB │ gzip:  0.30 kB
dist/assets/index-OipUvgmd.css   29.24 kB │ gzip:  5.42 kB
dist/assets/index-CeJp1IXy.js   120.83 kB │ gzip: 43.36 kB
✓ built in 118ms
```

`vue-tsc --noEmit` 无输出即无类型错误。

### 1.4 Clippy

**未执行，且不为此新装工具。** 仓库里没有 `.clippy.toml` 或 `src-tauri/.clippy.toml`，也没有任何 CI 步骤跑 clippy（`.github/workflows/build.yml` 与 `ci.yml` 均无 clippy 步骤）。按任务要求「若仓库有配置，跑一下；没有就算了」，判定为没有配置，跳过。

## 2. 未能验证的部分及原因

| 项目 | 为什么没做 |
|---|---|
| 知乎：打开登录会话 → 中文渲染 → `check_session` 返回 true | 需要真实知乎账号并登录，本项目没有。且任务明确禁止启动浏览器、登录真实平台。 |
| Dev.to：粘贴真实 API 令牌 → 校验 → 发布测试文章 → 确认线上可见 | 需要真实 dev.to 令牌，本项目没有；需要真实网络请求，任务明确禁止。 |
| 引擎解析链实际行为（首次下载 CfT 约 187 MB、之后复用、日志出现 `使用引擎: …（来源: Downloaded）`） | 需要启动应用并让引擎进程真正运行，任务明确禁止启动浏览器。本文中对「约 187 MB」「日志文案」的描述来自源码注释（`src-tauri/src/obscura/engine.rs:143-146`）与 `src-tauri/src/obscura/mod.rs:125`，**不是本次实测**。 |
| 知乎 / 小红书 / CSDN 发布页选择器是否命中 | 发布页选择器必须在登录态之后才能观察，需要真实账号。源码里这些常量本身就是**未实测的候选**（`src-tauri/src/platforms/zhihu.rs:26`、`:44`、`:50`）。 |
| 抖音 / 微信公众号会不会自动发布 | 理论上不需要账号——`publish: false` 的适配器在发布前校验阶段就会被拒绝。但触发这条路径需要运行应用并发起发布命令，属于任务禁止范围。该结论目前只有**单元测试**支撑：`platforms::capability_tests::validate_rejects_when_publish_is_unimplemented` 与 `unimplemented_publish_adapters_declare_no_capabilities` 通过（见 1.1）。 |
| 批量发布、cron 调度、掉号提醒在真实账号上的行为 | 需要真实账号与会话。 |
| 安装包（`MarketingConnect_*`）能否正常安装启动 | 本轮未运行 `tauri` 打包（任务禁止），也未在真实设备上校验 CI 产物。 |

> 关于「CI 只证明编译、打包、单元测试通过」这句话的精确含义：本轮我在**本机**执行了 1.1–1.3 三项，它们是同一套可复现的命令；本机未执行 `tauri` 打包。`.github/workflows/ci.yml`（功能分支）只做「编译 + 单测 + 前端构建」，**不含打包**；打包只发生在 `build.yml` 的 `tauri-action`，且该 workflow 仅在 `main` 与 `v*` 标签上触发。因此本轮的任何产物都**没有经过本机或 CI 的打包验证**。

## 3. 本次文档改动的事实依据（均在源码中逐条核对）

为保证 README 不夸大，以下说法都回查了源码而非照抄计划：

| 文档中的说法 | 源码位置 |
|---|---|
| 引擎解析顺序 `MC_ENGINE_CMD` → 本机 Chrome → 已下载 CfT → 自动下载 | `src-tauri/src/obscura/mod.rs:89-124`；`src-tauri/src/obscura/engine.rs:105-115` |
| 锁定版本 `157.0.8103.38` | `src-tauri/src/obscura/engine.rs:147`（`LOCKED_CFT_VERSION`） |
| 引擎安装路径 `engine/<版本>/chrome-<平台键>/` | `src-tauri/src/obscura/engine.rs:206-218`、`:167-198` |
| 每个实例独立 profile：`chrome-profile/<键>` | `src-tauri/src/obscura/engine.rs:282-294`；`src-tauri/src/obscura/mod.rs:153-155` |
| 日志 `使用引擎: …（来源: …）` | `src-tauri/src/obscura/mod.rs:125` |
| 中继丢弃日志文案 `丢弃无法解析的 CDP 事件（方法 {}）：继续转发会断开整条连接` | `src-tauri/src/obscura/relay.rs:173`（`grep -rn "丢弃无法解析的 CDP 事件" src-tauri/src/` 命中 `relay.rs:173`） |
| 响应帧永不丢弃 | `src-tauri/src/obscura/relay.rs:184-187`（`normalize`），对应测试 `never_drops_responses` |
| 两类版本差事件（`clientSecurityState` / 未知 `reason`） | `src-tauri/src/obscura/relay.rs:212-215`、`:261-291` |
| `page.get_cookies()` 受 URL 过滤 | `src-tauri/src/session.rs:220-224`（注释 `Network.getCookies via Page guarantees target isolation, at the cost of omitting cookies that do not match the current page/frame URLs.`） |
| Web Storage 只对 creator origin 恢复 | `src-tauri/src/session.rs:229-240`（`export_storage(&page, &origin)`，不匹配时 `warn!("页面来源不匹配，跳过 storage 导出")`） |
| `PlatformAdapter::login` 无调用方 | `src-tauri/src/platforms/zhihu.rs:106-111`；且 `cargo build` 警告 `method 'login' is never used --> src/platforms/mod.rs:157:14` |
| 交互式登录入口 `login_action` + `finish_account_login` | `src-tauri/src/login.rs:33`、`:84`；注册于 `src-tauri/src/lib.rs:112-113` |

### 3.1 能力矩阵（逐个适配器核对)

| 平台 | `publish` | `text` | `images` | `multi_image` | `video` | `cover` | `topics` | `thread` | `schedule` | 源码 |
|---|---|---|---|---|---|---|---|---|---|---|
| `douyin` 抖音 | **false** | false | false | false | false | false | false | false | false | `src-tauri/src/platforms/douyin.rs:43-51` |
| `xhls` 小红书 | true | true | false | false | false | false | false | false | false | `src-tauri/src/platforms/xiaohongshu.rs:48-57` |
| `wechat_mp` 公众号 | **false** | false | false | false | false | false | false | false | false | `src-tauri/src/platforms/wechat_mp.rs:57-65` |
| `csdn` | true | true | false | false | false | false | false | false | false | `src-tauri/src/platforms/csdn.rs:55-64` |
| `zhihu` 知乎 | true | true | **true** | **true** | false | **false** | **true** | false | false | `src-tauri/src/platforms/zhihu.rs:85-104` |
| `devto` | true | true | false | false | false | **false** | **true** | false | false | `src-tauri/src/platforms/devto.rs:94-104` |
| 自定义平台 | true | 取决于是否配置 `content_selector` | false | false | false | false | false | false | false | `src-tauri/src/platforms/custom.rs:63-75` |

`douyin` / `wechat_mp` 的 `publish()` 实测行为是直接 `bail!`（`douyin.rs` 与 `wechat_mp.rs` 的 `publish`），与能力位一致。`devto` 只有 `publish_with_token` 真正实现，浏览器 `publish()` 也直接 `bail!`（`src-tauri/src/platforms/devto.rs:125-131`），但令牌路径 `publish_with_token` 已实现（`:138-164`），因此 `publish: true` 成立。知乎 `media_paths` 走 `upload_any(MEDIA, ..)`，`MEDIA` 是通用 `input[type='file']`，代码无法区分图片与视频，按保守原则 `video: false`。

## 4. 验证边界的一句话总结

> 本轮**证明了**：代码可编译、单元测试通过、前端类型检查与打包通过。
> 本轮**没有证明**：任何真实平台上的登录、发布或选择器命中。所有「支持」均为「代码路径已实现」，不是「已在线上成功」。

## 5. 发现但**未修复**的代码问题（按要求只报告）

以下均在 `.rs` 层面，属于本任务禁止修改的范围，仅记录供后续处理：

1. **`src-tauri/src/obscura/relay.rs:3-15` 的模块文档与现状不符**：文档说「Obscura 的 `Network.requestWillBeSent` / `Network.responseReceived` 事件缺少字段」，但现在默认引擎是真 Chrome，该文件也承担了真 Chrome 157 的两类版本差修补；模块级注释仍只描述 Obscura，容易误导后来者。
2. **`cargo build` 有 10 个 warning**，其中 5 个是 `src/platforms/mod.rs:27-37` 的 `unused import`（`csdn::CsdnAdapter`、`CustomPlatformAdapter`、`douyin::DouyinAdapter`、`wechat_mp::WechatMpAdapter`、`xiaohongshu::XiaohongshuAdapter`），看起来是 `all_adapters` 改用全路径后遗留的导入。
3. **`src-tauri/src/mcp/tools.rs:27` 的工具描述仍是「查询 Obscura 浏览器引擎及实例状态」**，对 MCP 客户端暴露的文案未随引擎切换更新。
4. **`src-tauri/src/obscura/mod.rs:84`、`:193`、`:216`、`:231` 的日志仍写「启动/就绪/停止 obscura 实例」**，在真 Chrome 下的日志会让人误以为引擎是 Obscura。
5. **`src-tauri/src/obscura/mod.rs:256` 附近 `impl Drop for ObscuraManager` 的 `stop_all` 行为**：`accounts.rs:68` 的 `stop_engine` 会 `stop_all()` 停掉**所有**实例，MCP 的 `stop_engine` 工具（`mcp/tools.rs:183-185`）因此会停掉全部代理的引擎，而不只是目标实例——需确认是否符合预期。
6. **`.github/workflows/build.yml:46` 的注释**仍写「Windows x64（上游 Obscura 引擎暂无 arm64 Windows 构建）」，这个理由已不成立（CfT 有 `win-arm64`）。workflow 文件不在本任务允许修改的范围，故只记录。本任务所述的「CI 只证明编译、打包、单元测试通过」、`1.0.YYYYMMDD` 版本号与 ASCII 安装包名在本轮均未改动。
