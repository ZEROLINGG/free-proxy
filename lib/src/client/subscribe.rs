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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_url() {
        assert_eq!(
            subscribe_url("free-proxy.abc.workers.dev", false, 8080).unwrap(),
            "http://free-proxy.abc.workers.dev/subscribe/8080"
        );
    }

    #[test]
    fn https_url() {
        assert_eq!(
            subscribe_url("free-proxy.abc.workers.dev", true, 18080).unwrap(),
            "https://free-proxy.abc.workers.dev/subscribe/18080"
        );
    }

    #[test]
    fn empty_domain_rejected() {
        assert!(subscribe_url("", false, 8080).is_err());
    }

    #[test]
    fn trims_domain() {
        assert_eq!(
            subscribe_url("  free-proxy.abc.workers.dev  ", false, 8080).unwrap(),
            "http://free-proxy.abc.workers.dev/subscribe/8080"
        );
    }
}
