use anyhow::{anyhow, bail, Result};


use crate::test::BROWSER;
use crate::web::baseurl;

pub async fn proxy_example_com_http() -> Result<()> {
    let resp = BROWSER.get("http://example.com/").send().await?.text().await?;
    if resp.contains("Example Domain") {
        Ok(())
    } else {
        bail!("resp not contains 'Example Domain': {:.256}", resp.trim());
    }
}
pub async fn proxy_example_com_https() -> Result<()> {
    let resp = BROWSER.get("https://example.com/").send().await?.text().await?;
    if resp.contains("Example Domain") {
        Ok(())
    } else {
        bail!("resp not contains 'Example Domain': {:.256}", resp.trim());
    }
}

pub async fn proxy_localhost_hello() -> Result<()> {
    let resp = BROWSER.get(baseurl()).send().await?.text().await?;
    if resp.contains("Hello, World!") {
        Ok(())
    } else {
        bail!("resp not contains 'Hello, World!': {:.256}", resp.trim());
    }
}
