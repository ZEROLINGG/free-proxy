// 订阅链接生成:与 GUI Dashboard.tsx 同构
//   {scheme}://{domain}/subscribe/{port}
// 导入 Clash / sing-box / v2rayN 等客户端时,订阅服务端会自动按 UA 分发对应格式。
use anyhow::Result;

use crate::config::ProxySettings;

pub fn subscribe_url(s: &ProxySettings, port: u16) -> Result<String> {
    let scheme = if s.use_https { "https" } else { "http" };
    let domain = s.domain.trim();
    anyhow::ensure!(!domain.is_empty(), "domain(Worker 域名)不能为空");
    Ok(format!("{scheme}://{domain}/subscribe/{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> ProxySettings {
        let mut s = ProxySettings::defaults();
        s.domain = "free-proxy.abc.workers.dev".into();
        s
    }

    #[test]
    fn http_url() {
        let url = subscribe_url(&settings(), 8080).unwrap();
        assert_eq!(url, "http://free-proxy.abc.workers.dev/subscribe/8080");
    }

    #[test]
    fn https_url() {
        let mut s = settings();
        s.use_https = true;
        let url = subscribe_url(&s, 18080).unwrap();
        assert_eq!(url, "https://free-proxy.abc.workers.dev/subscribe/18080");
    }

    #[test]
    fn empty_domain_rejected() {
        let s = ProxySettings::defaults();
        assert!(subscribe_url(&s, 8080).is_err());
    }

    #[test]
    fn trims_domain() {
        let mut s = settings();
        s.domain = "  free-proxy.abc.workers.dev  ".into();
        assert_eq!(
            subscribe_url(&s, 8080).unwrap(),
            "http://free-proxy.abc.workers.dev/subscribe/8080"
        );
    }
}