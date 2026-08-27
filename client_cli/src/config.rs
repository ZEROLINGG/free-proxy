// 薄封装：复用 lib::client::config 为单一真源，保留原有导入路径兼容
pub use lib::client::config::{ProxySettings, app_data_dir, load, save};
