mod cs;
mod test;
mod util;
mod web;

use anyhow::Result;

use crate::cs::{Client, Server};
use crate::test::print_report;
use crate::web::WebServer;

#[allow(unused)]
use crate::test::{base::*, http::*};


#[tokio::main]
async fn main() -> Result<()> {
    let mut server = Server::new()?;
    server.start().await?;

    let mut client = Client::new(server.key().unwrap())?;
    client.start().await?;

    let web = WebServer::new();
    web.start().await?;

    let mut results = Vec::new();

    // ── 一次执行全部 46 个 E2E 用例 ──
    ensure_health!(server, results,
        // ── 基础链路 ──
        (proxy_example_com_http, 15),
        (proxy_example_com_https, 15),
        (proxy_localhost_hello),

        // ── 方法透传 ──
        (http_get_echo),
        (http_post_echo),
        (http_put_echo),
        (http_delete_echo),
        (http_patch_echo),
        (http_options_echo),

        // ── 状态码与特殊路径 ──
        (http_status_204),
        (http_status_304),
        (http_status_404),
        (http_status_500),
        (http_head_no_body),
        (http_redirect_chain),
        (http_multi_cookie),

        // ── 下载阶梯 ──
        (dl_0b),
        (dl_1b),
        (dl_1kb),
        (dl_64kb),
        (dl_256kb),
        (dl_1mb),
        (dl_5mb),
        (dl_10mb, 15),
        (dl_25mb, 20),
        (dl_30mb, 25),
        (dl_50mb, 30),

        // ── 上传阶梯 ──
        (ul_0b),
        (ul_1b),
        (ul_1kb),
        (ul_64kb),
        (ul_256kb),
        (ul_1mb),
        (ul_5mb),
        (ul_10mb, 15),
        (ul_25mb, 20),
        (ul_30mb, 25),
        (ul_50mb, 30),

        // ── 二进制对抗样本 ──
        (up_zeros_64k),
        (up_ff_64k),
        (up_delim_pattern),
        (up_random_64k),

        // ── chunked / 并发 ──
        (chunked_up_256kb),
        (concurrent_mixed_24),

        // ── 负向与gzip ──
        (negative_unconnectable_502),
        (gzip_body),
    );

    let ok = print_report(results);

    server.stop().await?;
    client.stop().await;
    web.stop().await?;

    if !ok {
        std::process::exit(1);
    }
    Ok(())
}