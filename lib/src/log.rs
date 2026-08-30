#[cfg(all(not(target_arch = "wasm32"), feature = "client"))]
use std::path::PathBuf;
use std::sync::OnceLock;
#[cfg(all(not(target_arch = "wasm32"), feature = "client"))]
use std::sync::{Arc, Mutex};

pub use tracing;

static TAG: OnceLock<String> = OnceLock::new();

pub fn set_tag<S: Into<String>>(p: S) {
    let _ = TAG.set(p.into());
}

#[doc(hidden)]
pub fn __get_tag() -> &'static str {
    TAG.get().map_or("", |p| p.as_str())
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::log::tracing::error!(
            target: module_path!(),
            "{} {}",
            $crate::log::__get_tag(),
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log::tracing::warn!(
            target: module_path!(),
            "{} {}",
            $crate::log::__get_tag(),
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log::tracing::info!(
            target: module_path!(),
            "{} {}",
            $crate::log::__get_tag(),
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::log::tracing::debug!(
            target: module_path!(),
            "{} {}",
            $crate::log::__get_tag(),
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::log::tracing::trace!(
            target: module_path!(),
            "{} {}",
            $crate::log::__get_tag(),
            format_args!($($arg)*)
        )
    };
}

// -----------------------------------------------------------------------------
// Native 客户端日志模块
// -----------------------------------------------------------------------------

/// 日志配置（native 客户端用）。
#[cfg(all(not(target_arch = "wasm32"), feature = "client"))]
#[derive(Clone, Debug)]
pub struct LogConfig {
    pub tag: String,
    /// None 表示仅终端 stderr。
    pub log_dir: Option<PathBuf>,
    /// 终端 ANSI 着色（GUI 落盘必须 false）。
    pub with_ansi: bool,
    /// RUST_LOG 未设置时兜底等级（如 "warn"、"debug"，支持模块级过滤语法）。
    pub default_level: String,
    /// 单文件字节上限（默认 1 MiB）。
    pub max_bytes: u64,
    /// 保留的轮转文件数（默认 3）。
    pub max_files: usize,
    /// 基础日志文件名（如 "freeproxy.log"）。
    pub file_name: String,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "client"))]
impl Default for LogConfig {
    fn default() -> Self {
        Self {
            tag: "".into(),
            log_dir: None,
            with_ansi: true,
            default_level: "warn".into(),
            max_bytes: 1024 * 1024,
            max_files: 3,
            file_name: "freeproxy.log".into(),
        }
    }
}

/// 输出目标（MakeWriter）：stderr 或 1MB 轮转文件。
#[cfg(all(not(target_arch = "wasm32"), feature = "client"))]
#[derive(Clone)]
enum Dest {
    Stderr,
    File(Arc<Mutex<rolling_file::RollingFileAppender<rolling_file::RollingConditionBasic>>>),
}

#[cfg(all(not(target_arch = "wasm32"), feature = "client"))]
enum DestWriter<'a> {
    Stderr(std::io::Stderr),
    File(
        std::sync::MutexGuard<
            'a,
            rolling_file::RollingFileAppender<rolling_file::RollingConditionBasic>,
        >,
    ),
}

#[cfg(all(not(target_arch = "wasm32"), feature = "client"))]
impl<'a> std::io::Write for DestWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            DestWriter::Stderr(w) => w.write(buf),
            DestWriter::File(guard) => guard.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            DestWriter::Stderr(w) => w.flush(),
            DestWriter::File(guard) => guard.flush(),
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "client"))]
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Dest {
    type Writer = DestWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        match self {
            Dest::Stderr => DestWriter::Stderr(std::io::stderr()),
            Dest::File(f) => {
                let guard = f.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                DestWriter::File(guard)
            }
        }
    }
}

/// 初始化日志（native 客户端：stderr 终端 + 可选 1MB 轮转文件）。
#[cfg(all(not(target_arch = "wasm32"), feature = "client"))]
pub fn init(cfg: LogConfig) -> std::io::Result<()> {
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt;

    set_tag(cfg.tag);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .and_then(|f| {
            Ok(f.add_directive("hyper=info".parse().unwrap())
                .add_directive("hyper_util=info".parse().unwrap())
                .add_directive("glycin=info".parse().unwrap())
            )
        })
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(&cfg.default_level)
                .add_directive("hyper=info".parse().unwrap())
                .add_directive("hyper_util=info".parse().unwrap())
                .add_directive("glycin=info".parse().unwrap())
        });

    let dest = match cfg.log_dir {
        Some(dir) => {
            std::fs::create_dir_all(&dir)?;
            let appender = rolling_file::RollingFileAppender::new(
                dir.join(cfg.file_name),
                rolling_file::RollingConditionBasic::new().max_size(cfg.max_bytes),
                cfg.max_files.max(1),
            )?;
            Dest::File(Arc::new(Mutex::new(appender)))
        }
        None => Dest::Stderr,
    };

    let layer = tracing_subscriber::fmt::layer()
        .with_writer(dest)
        .with_ansi(cfg.with_ansi)
        .compact()
        .with_thread_ids(false)
        .without_time()
        .with_filter(filter);

    let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer));
    Ok(())
}

// -----------------------------------------------------------------------------
// WASM 模块
// -----------------------------------------------------------------------------

/// 初始化日志（wasm worker：输出到控制台）。
#[cfg(target_arch = "wasm32")]
pub fn init_wasm(tag: impl Into<String>, default_level: impl AsRef<str>) {
    #[allow(unused)]
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt;

    set_tag(tag);

    let filter = tracing_subscriber::EnvFilter::try_new(default_level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));

    let mut builder = tracing_wasm::WASMLayerConfigBuilder::new();
    builder.set_report_logs_in_timings(false);
    builder.set_max_level(tracing::Level::TRACE);
    builder.set_console_config(tracing_wasm::ConsoleConfig::ReportWithConsoleColor);
    let layer = tracing_wasm::WASMLayer::new(builder.build());

    let _ = tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(layer).with(filter),
    );
}
