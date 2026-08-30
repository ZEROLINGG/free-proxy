pub mod proxy;
pub mod settings;
pub mod speed;

pub type Result<T> = std::result::Result<T, String>;

pub fn err_str(e: impl std::fmt::Display) -> String {
    // {e:#} 对 anyhow::Error 输出完整因果链；对普通 String 无副作用
    format!("{e:#}")
}
