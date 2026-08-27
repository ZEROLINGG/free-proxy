// 薄封装：复用 lib::client::ca 为单一真源，CLI 负责展示
#[allow(unused_imports)]
pub use lib::client::ca::{ca_key_secret, key_secret_for};

use anyhow::Result;

pub fn show_info() -> Result<()> {
    let info = lib::client::ca::ca_info()?;
    println!("CA 目录:   {}", info.dir.display());
    println!("证书路径: {}", info.path.display());
    if info.rebuilt {
        println!("⚠ 本次初始化检测到设备 UID 变化或密钥文件损坏,已自动重建 CA,");
        println!("  需要重新将证书导入系统信任区。");
    }
    println!("\n证书内容(PEM):");
    println!("{}", info.cert_pem);
    Ok(())
}

pub fn show_dir() -> Result<()> {
    let info = lib::client::ca::ca_info()?;
    println!("{}", info.dir.display());
    Ok(())
}

pub fn install() -> Result<()> {
    let report = lib::client::ca::install()?;
    println!("CA 证书已安装: {}", report.ca_path.display());
    if let Some(dest) = report.dest_path {
        println!("系统目标: {dest}");
    }
    for w in report.warnings {
        println!("警告: {w}");
    }
    Ok(())
}
