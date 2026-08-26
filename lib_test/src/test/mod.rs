#![allow(unused)]
use reqwest::{Client, ClientBuilder};
use std::sync::LazyLock;
use std::time::Duration;
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



pub type TestResult = (String, TestResultType); // 函数名，结果
pub enum TestResultType {
    Success(Duration),
    Failure(Duration, String), // 耗时和详细信息
    Panic(Duration, String),
    Timeout(Duration, String),
}

/// 内部实现：带超时的测试执行（超时即 abort 任务并记为 Timeout）
#[macro_export]
macro_rules! test_fn {
    ($fn_name:ident, $results_vec:expr, $timeout_secs:expr) => {
        let start_time = std::time::Instant::now();
        let test_name = stringify!($fn_name);

        // ANSI 颜色与样式定义
        let cyan = "\x1b[1;36m";
        let green = "\x1b[1;32m";
        let red = "\x1b[1;31m";
        let dim = "\x1b[90m";
        let reset = "\x1b[0m";

        println!("\n{green}======================================================================{reset}");
        println!("{cyan}[RUNNING TEST]:{reset} {test_name}");
        println!("{dim}----------------------------------------------------------------------{reset}");

        // 使用 tokio::spawn 开启独立任务，隔离 panic 风险
        let mut handle = ::tokio::spawn(async move {
            $fn_name().await
        });

        match ::tokio::time::timeout(::tokio::time::Duration::from_secs($timeout_secs), &mut handle).await {
            // 超时：abort 悬挂任务，防止断链时等满 96s idle
            Err(_) => {
                handle.abort();
                let duration = start_time.elapsed();
                let err_msg = format!("test timed out after {}s", $timeout_secs);
                println!("{dim}----------------------------------------------------------------------{reset}");
                println!("{red}[TIMEOUT]:{reset} {test_name} {dim}(took {duration:.2?}){reset}");
                println!("{red}┌─ Timeout Details ───────────────────────────────────────────────────{reset}");
                println!("{red}│{reset} {err_msg}");
                println!("{red}└─────────────────────────────────────────────────────────────────────{reset}");
                $results_vec.push((test_name.to_string(), $crate::test::TestResultType::Timeout(duration, err_msg)));
            }
            // 1. 任务正常完成，且业务逻辑返回 Ok
            Ok(Ok(Ok(_))) => {
                let duration = start_time.elapsed();
                println!("{dim}----------------------------------------------------------------------{reset}");
                println!(
                    "{green}[PASSED]:{reset} {test_name} {dim}(took {:.2?}){reset}",
                    duration
                );

                // 将成功结果推入外部 Vec
                $results_vec.push((test_name.to_string(), $crate::test::TestResultType::Success(duration)));
            }
            // 2. 任务正常完成，但业务逻辑返回了 Err(e)
            Ok(Ok(Err(e))) => {
                let duration = start_time.elapsed();
                let err_msg = format!("{e}");

                println!("{dim}----------------------------------------------------------------------{reset}");
                println!(
                    "{red}[FAILED]:{reset} {test_name} {dim}(took {:.2?}){reset}",
                    duration
                );
                println!("{red}┌─ Error Details ─────────────────────────────────────────────────────{reset}");
                for line in err_msg.lines() {
                    println!("{red}│{reset} {line}");
                }
                println!("{red}└─────────────────────────────────────────────────────────────────────{reset}");

                // 将失败结果推入外部 Vec
                $results_vec.push((test_name.to_string(), $crate::test::TestResultType::Failure(duration, err_msg)));
            }
            // 3. 任务本身崩溃
            Ok(Err(join_err)) => {
                let duration = start_time.elapsed();
                println!("{dim}----------------------------------------------------------------------{reset}");
                println!(
                    "{red}[PANICKED]:{reset} {test_name} {dim}(took {:.2?}){reset}",
                    duration
                );
                println!("{red}┌─ Panic Details ─────────────────────────────────────────────────────{reset}");

                let msg = if join_err.is_panic() {
                    let panic_err = join_err.into_panic();
                    // 尝试向下转型提取具体的 panic 文本信息
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

                for line in msg.lines() {
                    println!("{red}│{reset} {line}");
                }
                println!("{red}└─────────────────────────────────────────────────────────────────────{reset}");

                // 将 Panic 结果推入外部 Vec
                $results_vec.push((test_name.to_string(), $crate::test::TestResultType::Panic(duration, msg)));
            }
        }
        println!("{green}======================================================================{reset}");
    };

    ($fn_name:ident, $results_vec:expr) => {
        $crate::test_fn!($fn_name, $results_vec, 10);
    };
}


/// 打印最终的测试报告
pub fn print_report(tests: Vec<TestResult>) {
    let cyan = "\x1b[1;36m";
    let green = "\x1b[1;32m";
    let red = "\x1b[1;31m";
    let dim = "\x1b[90m";
    let bold = "\x1b[1m";
    let yellow = "\x1b[1;33m";
    let reset = "\x1b[0m";

    let total = tests.len();
    if total == 0 {
        println!("{yellow}No tests were run.{reset}");
        return;
    }

    let mut passed = 0;
    let mut failed = 0;
    let mut panicked = 0;
    let mut timed_out = 0;
    let mut total_duration = Duration::new(0, 0);

    // 统计结果数据
    for (_, result_type) in &tests {
        match result_type {
            TestResultType::Success(d) => {
                passed += 1;
                total_duration += *d;
            }
            TestResultType::Failure(d, _) => {
                failed += 1;
                total_duration += *d;
            }
            TestResultType::Panic(d, _) => {
                panicked += 1;
                total_duration += *d;
            }
            TestResultType::Timeout(d, _) => {
                timed_out += 1;
                total_duration += *d;
            }
        }
    }

    // 打印总结头部
    println!("\n{cyan}=========================== TEST REPORT =============================={reset}");
    println!("  Total Tests : {bold}{total}{reset}");
    println!("  Passed      : {green}{bold}{passed}{reset}");
    println!("  Failed      : {red}{bold}{failed}{reset}");
    println!("  Panicked    : {red}{bold}{panicked}{reset}");
    println!("  Timed out   : {yellow}{bold}{timed_out}{reset}");
    println!("  Total Time  : {dim}{total_duration:.2?}{reset}");
    println!("{cyan}======================================================================{reset}");

    // 打印失败/崩溃/超时的具体细节（如果有的话）
    if failed > 0 || panicked > 0 || timed_out > 0 {
        println!("\n{bold}Issues Breakdown:{reset}");
        for (name, result_type) in tests {
            match result_type {
                TestResultType::Failure(d, msg) => {
                    println!("  {red} FAILED{reset}   {name} {dim}(took {d:.2?}){reset}");
                    println!("    {dim}Reason:{reset} {:.1024}", msg.replace("\n", "\n            ")); // 缩进处理对齐
                }
                TestResultType::Panic(d, msg) => {
                    println!("  {red} PANICKED{reset} {name} {dim}(took {d:.2?}){reset}");
                    println!("    {dim}Reason:{reset} {:.1024}", msg.replace("\n", "\n            ")); // 缩进处理对齐
                }
                TestResultType::Timeout(d, msg) => {
                    println!("  {yellow} TIMED OUT{reset} {name} {dim}(took {d:.2?}){reset}");
                    println!("    {dim}Reason:{reset} {:.1024}", msg.replace("\n", "\n            "));
                }
                _ => {}
            }
        }
        println!();
    } else {
        println!("\n{green}All tests passed successfully! {reset}");
    }
}