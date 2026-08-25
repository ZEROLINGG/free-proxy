
mod cs;
mod test;
mod util;
mod web;

use std::time::Duration;
use anyhow::Result;
use tokio::time::sleep;
use crate::cs::{Client, Server};
use crate::test::base::*;
use crate::test::http::*;
use crate::test::print_report;
use crate::web::WebServer;

#[tokio::main]
async fn main() -> Result<()> {
    let mut server = Server::new()?;
    server.start().await?;

    let mut client = Client::new(server.key().unwrap())?;
    client.start().await?;

    let web = WebServer::new();
    web.start().await?;


    let _ = dbg!(client.check_availability().await);
    sleep(Duration::from_secs(5)).await;
    let mut results = Vec::new();

    // ── 基础链路 ──
    test_fn!(proxy_example_com_http, results);
    test_fn!(proxy_example_com_https, results);
    test_fn!(proxy_localhost_hello, results);

    // ── 方法透传 ──
    test_fn!(http_get_echo, results);
    test_fn!(http_post_echo, results);
    test_fn!(http_put_echo, results);
    test_fn!(http_delete_echo, results);
    test_fn!(http_patch_echo, results);
    test_fn!(http_options_echo, results);

    // ── 状态码与特殊路径 ──
    test_fn!(http_status_204, results);
    test_fn!(http_status_304, results);
    test_fn!(http_status_404, results);
    test_fn!(http_status_500, results);
    test_fn!(http_head_no_body, results);
    test_fn!(http_redirect_chain, results);
    test_fn!(http_multi_cookie, results);

    // ── 大小阶梯：下载 ──
    test_fn!(dl_0b, results);
    test_fn!(dl_1b, results);
    test_fn!(dl_1kb, results);
    test_fn!(dl_64kb, results);
    test_fn!(dl_256kb, results);
    test_fn!(dl_1mb, results);
    test_fn!(dl_5mb, results);
    test_fn!(dl_10mb, results);
    test_fn!(dl_25mb, results);

    // ── 大小阶梯：上传 ──
    test_fn!(ul_0b, results);
    test_fn!(ul_1b, results);
    test_fn!(ul_1kb, results);
    test_fn!(ul_64kb, results);
    test_fn!(ul_256kb, results);
    test_fn!(ul_1mb, results);
    test_fn!(ul_5mb, results);
    test_fn!(ul_10mb, results);
    test_fn!(ul_25mb, results);

    // ── 二进制对抗样本 ──
    test_fn!(up_zeros_64k, results);
    test_fn!(up_ff_64k, results);
    test_fn!(up_delim_pattern, results);
    test_fn!(up_random_64k, results);

    // ── chunked 上传 / keep-alive / 并发 / 慢速流 ──
    test_fn!(chunked_up_256kb, results);
    test_fn!(keepalive_seq_6, results);
    test_fn!(concurrent_mixed_8, results);
    test_fn!(slow_download_32kb, results);

    // ── 负向与特征化 ──
    test_fn!(negative_unconnectable_502, results);
    test_fn!(charz_gzip_passthrough, results);




    print_report(results);

    Ok(())
}
