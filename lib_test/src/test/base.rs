use anyhow::{anyhow, bail, Result};


use crate::test::BROWSER;
use crate::web::baseurl;

async fn proxy_get_match(url: &str, substring: &str) -> Result<()> {
    let resp = BROWSER.get(url).send().await?;
    let code = resp.status();
    if let Ok(body) = resp.text().await {
        if !body.is_empty() {
            if body.contains(substring) {
                return Ok(());
            } else {
                bail!("body not contains '{substring}' [StatusCode:{code:?}]: {:.256}", body.trim());
            }
        } else { bail!("body is empty [StatusCode:{code:?}]"); }
    }
    bail!("resp.text() error [StatusCode:{code:?}]");
}

pub async fn proxy_example_com_http() -> Result<()> {
    proxy_get_match("http://example.com/","Example Domain").await
}
pub async fn proxy_example_com_https() -> Result<()> {
    proxy_get_match("https://example.com/","Example Domain").await
}

pub async fn proxy_localhost_hello() -> Result<()> {
    proxy_get_match(baseurl(),"Hello, World!").await
}
