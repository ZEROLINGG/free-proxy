# E2E Worker 崩溃熔断 + 环境收敛

## 背景结论(分析摘要)

- 本次 test-e2e 44 例中 25 个失败/超时,真实故障只有 2 个:
  1. 根因 A:workerd/miniflare 本地出站 fetch 间歇性抛 "Network connection lost"(命中 server-rs/src/proxy_http.rs:231 的 Fetch::send() 失败分支 → 502),dev 回环竞态,生产不存在;
  2. 根因 B:dl_5mb 期间 wrangler/workerd 进程整体崩溃(exit 1),其后所有失败(含 ul_256kb/1mb/5mb 三连超时)均为"127.0.0.1:80 连接拒绝"级联噪声;崩溃前 19 例全绿证明协议链路正确。
- 环境漂移放大因素:根目录无 node_modules(wrangler 全局安装、版本漂移:运行时 4.125.0 vs 当前全局 4.120.0)、Node v24 vs CI 规定 Node 22。
- harness 缺口(lib_test/src/cs.rs):就绪检测仅 TCP connect + sleep 5s(不用现成 /health 路由);不监控子进程存活,崩溃后盲跑剩余约 30 例污染报告。

## 改动清单

### P0 — 根目录工具链固定

1. package.json(仓库根)增加:

   ```json
   "engines": { "node": ">=22" },
   "devDependencies": { "wrangler": "4.125.0" }
   ```

   (4.125.0 = 本次运行版本 = npm latest,精确锁定)
2. 根目录执行 pnpm install → 生成 node_modules/ 与 pnpm-lock.yaml;此后 pnpm server-dev 经 PATH 优先解析到本地锁定版 wrangler,消除全局漂移。

### P1a — lib_test/src/cs.rs

1. 新增进程状态与辅助函数:

   ```rust
   static WRANGLER_EXIT_CODE: AtomicI32 = AtomicI32::new(i32::MIN);
   static CRASH_BANNER_SHOWN: AtomicBool = AtomicBool::new(false);
   pub fn wrangler_dead() -> bool;
   pub fn wrangler_exit_code() -> i32;
   pub fn take_crash_banner() -> bool; // 首个调用者返回 true,用于横幅去重
   ```

2. Shell builder 链(cs.rs:84)追加 .on_exit 回调:shell-engine 对自然退出/崩溃触发 fire_exit(外部 kill 不触发,不影响 stop());回调内 store 退出码并 eprintln 提示。
3. 就绪检测:替换 cs.rs:102-108 的 TCP connect + 固定 sleep:
   - derive_keys(key, "127.0.0.1") → gen_auth_token(token_base)(lib::tool;lib_test 默认启用 client feature,可用);
   - reqwest no_proxy client(单次 3s 超时)GET http://127.0.0.1/health + bearer_auth,校验 200 且 body 可解析为 u64(/health 在 auth 中间件之后,需带 token;app.rs:47);
   - 连续 3 次成功才算就绪;每秒轮询,总预算保持原 ~45min;轮询期间若 wrangler_dead() 提前 bail。

### P1b — lib_test/src/test/mod.rs

1. TestResultType 增加 Skipped(Duration, String) 变体。
2. test_fn_timeout! 宏开头增加熔断分支:若 crate::cs::wrangler_dead() → 用 take_crash_banner() 去重打印一次 FATAL 横幅(含 exit code),打印 [SKIPPED] 行,push Skipped 结果后直接进入下一用例(现有 spawn/timeout 逻辑包进 else 分支)。main.rs 用例清单零改动。
3. print_report:统计区加 Skipped 计数行;breakdown 增加 SKIPPED 条目输出;末尾若 crate::cs::wrangler_dead() 追加一行说明"SKIPPED 因 wrangler 崩溃,非用例本身问题"。

## 明确不做

- 不改 server-rs / lib 产品代码(dev 竞态重试等 P2 缓解,待崩溃根因确认后再议);
- 不改 .dev.vars 覆盖行为(既有已知设计)。

## 验证

1. cd lib_test && cargo check 编译通过;
2. npm run test-e2e 复跑:预期 44 例全 PASS(注意会重写 server-rs/.dev.vars 为随机 key,属既有已知行为);
3. 熔断注入验证:e2e 运行中另开终端 pkill -f "wrangler dev",确认从下一个用例起全部 SKIPPED、FATAL 横幅只打一次、最终报告标注 wrangler 已退出。
