pub mod proxy;
pub mod settings;
pub mod speed;

pub type Result<T> = std::result::Result<T, String>;

pub fn err_str(e: impl std::fmt::Display) -> String {
    format!("{e}")
}
