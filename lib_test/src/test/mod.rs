#![allow(unused)]
use reqwest::{Client, ClientBuilder};
use std::sync::LazyLock;
use std::time::Duration;
use std::cmp::Ordering;
use std::collections::HashMap;
use crate::cs::proxy_url;

pub mod base;
pub mod http;

pub static BROWSER: LazyLock<Client> = LazyLock::new(|| {
    ClientBuilder::new()
        .proxy(reqwest::Proxy::all(proxy_url()).unwrap())
        .danger_accept_invalid_certs(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .build()
        .unwrap()
});

pub type TestResult = (String, Vec<TestResultType>); // 函数名，多次执行的结果

#[derive(Clone)]
pub enum TestResultType {
    Success(Duration),
    Failure(Duration, String), // 耗时和详细信息
    Panic(Duration, String),
    Timeout(Duration, String),
}

/// 内部实现：带超时和重复次数的测试执行
#[macro_export]
macro_rules! test_fn {
    // 完整签名：指定超时时间和重复次数
    ($fn_name:ident, $results_vec:expr, $hook:block, $timeout_secs:expr, $repeat_count:expr) => {
        let test_name = stringify!($fn_name);

        // ANSI 颜色与样式定义
        let cyan = "\x1b[1;36m";
        let green = "\x1b[1;32m";
        let red = "\x1b[1;31m";
        let yellow = "\x1b[1;33m";
        let dim = "\x1b[90m";
        let reset = "\x1b[0m";

        println!("\n{green}======================================================================{reset}");
        println!("{cyan}[RUNNING TEST]:{reset} {test_name} {dim}({} runs, {}s timeout){reset}", $repeat_count, $timeout_secs);
        println!("{dim}----------------------------------------------------------------------{reset}");

        let mut run_results = Vec::with_capacity($repeat_count);

        for run_idx in 1..=$repeat_count {
            $hook
            let start_time = std::time::Instant::now();
            println!("{cyan}[TEST_START]{reset} Start {}/{}", run_idx, $repeat_count);

            // 使用 tokio::spawn 开启独立任务，隔离 panic 风险
            let mut handle = ::tokio::spawn(async move {
                $fn_name().await
            });

            match ::tokio::time::timeout(::tokio::time::Duration::from_secs($timeout_secs), &mut handle).await {
                // 1. 超时：abort 悬挂任务
                Err(_) => {
                    handle.abort();
                    let duration = start_time.elapsed();
                    let err_msg = format!("test timed out after {}s", $timeout_secs);
                    println!("{yellow}[TEST_TIMEOUT]{reset} Run {}/{} {dim}(took {duration:.2?}){reset}", run_idx, $repeat_count);
                    run_results.push($crate::test::TestResultType::Timeout(duration, err_msg));
                }
                // 2. 任务正常完成，且业务逻辑返回 Ok
                Ok(Ok(Ok(_))) => {
                    let duration = start_time.elapsed();
                    println!("{green}[TEST_PASSED]{reset}  Run {}/{} {dim}(took {:.2?}){reset}", run_idx, $repeat_count, duration);
                    run_results.push($crate::test::TestResultType::Success(duration));
                }
                // 3. 任务正常完成，但业务逻辑返回了 Err(e)
                Ok(Ok(Err(e))) => {
                    let duration = start_time.elapsed();
                    let err_msg = format!("{e}");
                    println!("{red}[TEST_FAILED]{reset}  Run {}/{} {dim}(took {:.2?}){reset}  {err_msg:.512}", run_idx, $repeat_count, duration);
                    run_results.push($crate::test::TestResultType::Failure(duration, err_msg));
                }
                // 4. 任务本身崩溃
                Ok(Err(join_err)) => {
                    let duration = start_time.elapsed();
                    let err_msg = if join_err.is_panic() {
                        let panic_err = join_err.into_panic();
                        if let Some(s) = panic_err.downcast_ref::<&str>() {
                            s.to_string()
                        } else if let Some(s) = panic_err.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "Unknown panic payload".to_string()
                        }
                    } else if join_err.is_cancelled() {
                        "Task was unexpectedly cancelled.".to_string()
                    } else {
                        format!("Unknown JoinError: {:?}", join_err)
                    };
                    println!("{red}[TEST_PANICKED]{reset} Run {}/{} {dim}(took {:.2?}){reset}  {err_msg:.512}", run_idx, $repeat_count, duration);
                    run_results.push($crate::test::TestResultType::Panic(duration, err_msg));
                }
            }
        }
        println!("{green}======================================================================{reset}");

        // 将包含所有轮次结果的数组推入外部 Vec
        $results_vec.push((test_name.to_string(), run_results));
    };

    ($fn_name:ident, $results_vec:expr, $hook:block) => {
        $crate::test_fn!($fn_name, $results_vec, $hook, 10, 4);
    };
    ($fn_name:ident, $results_vec:expr) => {
        $crate::test_fn!($fn_name, $results_vec, {}, 10, 4);
    };
}
#[macro_export]
macro_rules! ensure_health {
    // 入口：捕获 server, results 和若干测试元组
    ($server:expr, $results:expr, $($test:tt),* $(,)?) => {
        $(
            $crate::ensure_health!(@test $server, $results, $test);
        )*
    };

    // 仅函数名：使用默认超时和重复次数
    (@test $server:expr, $results:expr, ($fn:ident)) => {
        $crate::test_fn!($fn, $results, { $server.ensure_health().await? });
    };

    // 函数名 + 超时：重复次数固定为 5
    (@test $server:expr, $results:expr, ($fn:ident, $timeout:expr)) => {
        $crate::test_fn!($fn, $results, { $server.ensure_health().await? }, $timeout, 4);
    };

    // 函数名 + 超时 + 重复次数
    (@test $server:expr, $results:expr, ($fn:ident, $timeout:expr, $repeat:expr)) => {
        $crate::test_fn!($fn, $results, { $server.ensure_health().await? }, $timeout, $repeat);
    };
}


/// 测试函数稳定性分类
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]  // 添加 Hash
enum Stability {
    StableSuccess,      // 全部成功
    UnstableSuccess,    // 成功过半
    UnstablePanic,      // Panic 数量最多（或并列优先）
    UnstableFailure,    // Failure 数量最多（Panic 并列时被覆盖）
    UnstableTimeout,    // Timeout 数量最多（Panic/Failure 并列时被覆盖）
    StablePanic,        // 全部 Panic
    StableFailure,      // 全部 Failure
    StableTimeout,      // 全部 Timeout
}

impl Stability {
    fn label(&self) -> &'static str {
        match self {
            Stability::StableSuccess => "Stable Success",
            Stability::UnstableSuccess => "Unstable Success",
            Stability::UnstablePanic => "Unstable Panic",
            Stability::UnstableFailure => "Unstable Failure",
            Stability::UnstableTimeout => "Unstable Timeout",
            Stability::StablePanic => "Stable Panic",
            Stability::StableFailure => "Stable Failure",
            Stability::StableTimeout => "Stable Timeout",
        }
    }

    fn color_code(&self) -> &'static str {
        match self {
            Stability::StableSuccess => "\x1b[1;32m",   // bold green
            Stability::UnstableSuccess => "\x1b[1;33m", // bold yellow
            Stability::UnstablePanic | Stability::StablePanic => "\x1b[1;31m", // bold red
            Stability::UnstableFailure | Stability::StableFailure => "\x1b[1;31m",
            Stability::UnstableTimeout | Stability::StableTimeout => "\x1b[1;33m",
        }
    }
}

/// 函数级别的报告数据
struct FuncReport {
    name: String,
    success_count: usize,
    panic_count: usize,
    failure_count: usize,
    timeout_count: usize,
    total: usize,
    classification: Stability,
    success_rate: f64,        // 0.0 ~ 1.0
    median_duration: Duration,
    all_durations: Vec<Duration>, // 保留用于中位数计算（已排序）
    failed_runs: Vec<(usize, TestResultType)>, // 轮次索引（从1开始）和对应的非Success结果
}

impl FuncReport {
    fn from_test_result((name, results): &TestResult) -> Self {
        let name = name.clone();
        let total = results.len();
        let mut success_count = 0;
        let mut panic_count = 0;
        let mut failure_count = 0;
        let mut timeout_count = 0;
        let mut durations = Vec::with_capacity(total);
        let mut failed_runs = Vec::new();

        for (i, r) in results.iter().enumerate() {
            let idx = i + 1;
            match r {
                TestResultType::Success(d) => {
                    success_count += 1;
                    durations.push(*d);
                }
                TestResultType::Panic(d, _msg) => {
                    panic_count += 1;
                    durations.push(*d);
                    failed_runs.push((idx, r.clone())); // 直接 clone，无需解引用
                }
                TestResultType::Failure(d, _msg) => {
                    failure_count += 1;
                    durations.push(*d);
                    failed_runs.push((idx, r.clone()));
                }
                TestResultType::Timeout(d, _msg) => {
                    timeout_count += 1;
                    durations.push(*d);
                    failed_runs.push((idx, r.clone()));
                }
            }
        }

        // 计算中位数
        durations.sort();
        let median = if total == 0 {
            Duration::from_secs(0)
        } else if total % 2 == 1 {
            durations[total / 2]
        } else {
            let a = durations[total / 2 - 1];
            let b = durations[total / 2];
            // 取平均值（以纳秒计）
            let avg_ns = (a.as_nanos() + b.as_nanos()) / 2;
            Duration::from_nanos(avg_ns as u64)
        };

        // 确定分类
        let classification = if success_count == total {
            Stability::StableSuccess
        } else if panic_count == total {
            Stability::StablePanic
        } else if failure_count == total {
            Stability::StableFailure
        } else if timeout_count == total {
            Stability::StableTimeout
        } else if success_count > total / 2 {
            Stability::UnstableSuccess
        } else {
            // 找出错误类型数量最多的（按 Panic > Failure > Timeout 优先级）
            let max_count = panic_count.max(failure_count).max(timeout_count);
            if panic_count == max_count {
                Stability::UnstablePanic
            } else if failure_count == max_count {
                Stability::UnstableFailure
            } else {
                Stability::UnstableTimeout
            }
        };

        let success_rate = if total > 0 {
            success_count as f64 / total as f64
        } else {
            0.0
        };

        FuncReport {
            name,
            success_count,
            panic_count,
            failure_count,
            timeout_count,
            total,
            classification,
            success_rate,
            median_duration: median,
            all_durations: durations,
            failed_runs,
        }
    }
}

/// 打印优化后的综合测试与稳定性报告；返回门禁判定：
/// true = 所有函数均为 稳定成功/不稳定成功（通过）；false = 存在任何失败类别或空结果。
pub fn print_report(tests: Vec<TestResult>) -> bool {
    let cyan = "\x1b[1;36m";
    let green = "\x1b[1;32m";
    let red = "\x1b[1;31m";
    let dim = "\x1b[90m";
    let bold = "\x1b[1m";
    let yellow = "\x1b[1;33m";
    let reset = "\x1b[0m";

    if tests.is_empty() {
        println!("{yellow}No tests were run.{reset}");
        return false;
    }

    // 1. 生成每个函数的报告
    let mut reports: Vec<FuncReport> = tests.iter().map(FuncReport::from_test_result).collect();

    // 2. 整体统计
    let total_functions = reports.len();
    let mut total_runs = 0;
    let mut total_success = 0;
    let mut total_panic = 0;
    let mut total_failure = 0;
    let mut total_timeout = 0;
    let mut total_duration = Duration::ZERO;

    let mut classification_counts = HashMap::new();
    for rep in &reports {
        total_runs += rep.total;
        total_success += rep.success_count;
        total_panic += rep.panic_count;
        total_failure += rep.failure_count;
        total_timeout += rep.timeout_count;
        // 累加所有耗时（为了总耗时）
        for d in &rep.all_durations {
            total_duration += *d;
        }
        *classification_counts.entry(rep.classification).or_insert(0) += 1;
    }

    // 3. 打印头部汇总
    println!("\n{cyan}=========================== TEST REPORT =============================={reset}");
    println!("  Total Functions : {bold}{total_functions}{reset}");
    println!("  Total Runs      : {bold}{total_runs}{reset}");
    println!("  Passed Runs     : {green}{bold}{total_success}{reset}");
    println!("  Failed Runs     : {red}{bold}{total_failure}{reset}");
    println!("  Panicked Runs   : {red}{bold}{total_panic}{reset}");
    println!("  Timed Out Runs  : {yellow}{bold}{total_timeout}{reset}");
    println!("  Total Time      : {dim}{total_duration:.2?}{reset}");
    println!("{cyan}======================================================================{reset}");

    // 4. 分类统计摘要
    println!("\n{bold}Stability Classification Summary:{reset}");
    let all_categories = [
        Stability::StableSuccess,
        Stability::UnstableSuccess,
        Stability::UnstablePanic,
        Stability::UnstableFailure,
        Stability::UnstableTimeout,
        Stability::StablePanic,
        Stability::StableFailure,
        Stability::StableTimeout,
    ];
    for cat in all_categories.iter() {
        if let Some(count) = classification_counts.get(cat) {
            let color = cat.color_code();
            println!("  {color}{:<20}{reset} : {bold}{}{reset}", cat.label(), count);
        }
    }
    println!();

    // 5. 每个函数的详细表格
    println!("{cyan}---------------------------- Function Details ---------------------------{reset}");
    // 表头
    println!(
        "{:<30} {:<20} {:>10} {:>12}",
        "Function Name", "Classification", "Success %", "Median Time"
    );
    println!("{}-{}-{}-{}", "-".repeat(30), "-".repeat(20), "-".repeat(12), "-".repeat(14));

    // 按分类排序：先稳定成功，再不稳定成功，再其他
    reports.sort_by_key(|r| {
        match r.classification {
            Stability::StableSuccess => 0,
            Stability::UnstableSuccess => 1,
            Stability::UnstablePanic => 2,
            Stability::UnstableFailure => 3,
            Stability::UnstableTimeout => 4,
            Stability::StablePanic => 5,
            Stability::StableFailure => 6,
            Stability::StableTimeout => 7,
        }
    });

    for rep in &reports {
        let color = rep.classification.color_code();
        let rate = rep.success_rate * 100.0;
        println!(
            "{color}{:<30}{reset} {:<20} {:>9.1}%  {:>12?}",
            rep.name,
            rep.classification.label(),
            rate,
            rep.median_duration
        );
    }
    println!("{cyan}----------------------------------------------------------------------{reset}");

    // 6. 问题详情（仅显示存在失败轮次的函数）
    let has_issues = reports.iter().any(|r| !r.failed_runs.is_empty() && r.classification != Stability::StableSuccess);
    if has_issues {
        println!("\n{bold}Issues Breakdown (Non-Successful Runs):{reset}");
        for rep in &reports {
            if rep.failed_runs.is_empty() || rep.classification == Stability::StableSuccess {
                continue;
            }
            let status_tag = format!("{}{}{reset}",rep.classification.color_code(),rep.classification.label().to_uppercase());

            println!(
                "\n  [{status_tag}] {bold}{}{reset} {dim}(Passed: {}/{}){reset}",
                rep.name, rep.success_count, rep.total
            );

            for (run_idx, result) in &rep.failed_runs {
                match result {
                    TestResultType::Failure(d, msg) => {
                        println!("    {red}Run {run_idx}: FAILED{reset} {dim}(took {d:.2?}){reset}");
                        println!("      {dim}Reason:{reset} {:.1024}", msg.replace('\n', "\n            "));
                    }
                    TestResultType::Panic(d, msg) => {
                        println!("    {red}Run {run_idx}: PANICKED{reset} {dim}(took {d:.2?}){reset}");
                        println!("      {dim}Reason:{reset} {:.1024}", msg.replace('\n', "\n            "));
                    }
                    TestResultType::Timeout(d, msg) => {
                        println!("    {yellow}Run {run_idx}: TIMED OUT{reset} {dim}(took {d:.2?}){reset}");
                        println!("      {dim}Reason:{reset} {:.1024}", msg.replace('\n', "\n            "));
                    }
                    _ => unreachable!(),
                }
            }
        }
        println!();
    } else {
        println!("\n{green}All tests passed consistently! 100% Stability.{reset}\n");
    }

    // 门禁判定：仅 稳定成功 / 不稳定成功 视为通过，其余（含空）一律失败
    reports.iter().all(|r| {
        matches!(
            r.classification,
            Stability::StableSuccess | Stability::UnstableSuccess
        )
    })
}
