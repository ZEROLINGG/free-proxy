use anyhow::Result;

pub fn subscribe_url(domain: &str, use_https: bool, port: u16) -> Result<String> {
    let scheme = if use_https { "https" } else { "http" };
    let domain = domain.trim();
    anyhow::ensure!(!domain.is_empty(), "domain(Worker 域名)不能为空");
    Ok(format!("{scheme}://{domain}/subscribe/{port}"))
}

pub fn subscribe_url_from_settings(domain: &str, use_https: bool, port: u16) -> Result<String> {
    subscribe_url(domain, use_https, port)
}

