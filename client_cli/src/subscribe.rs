use anyhow::Result;
use crate::config::ProxySettings;

pub fn subscribe_url(s: &ProxySettings, port: u16) -> Result<String> {
    lib::client::subscribe::subscribe_url(&s.domain, s.use_https, port)
}
