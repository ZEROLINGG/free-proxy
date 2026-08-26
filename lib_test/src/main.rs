
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

    // 一次性执行所有测试
    ensure_health!(server, results,
        // // 基础链路
        // // ---------------------------- Function Details ---------------------------
        // // Function Name                  Classification        Success %  Median Time
        // // -------------------------------------------------------------------------------
        // // proxy_example_com_http         Stable Success           100.0%  2.196385826s
        // // proxy_example_com_https        Stable Success           100.0%  173.629723ms
        // // proxy_localhost_hello          Stable Success           100.0%   12.088535ms
        // (proxy_example_com_http, 15),
        // (proxy_example_com_https, 15),
        // (proxy_localhost_hello),
        //
        //
        // // 方法透传
        // // ---------------------------- Function Details ---------------------------
        // // Function Name                  Classification        Success %  Median Time
        // // -------------------------------------------------------------------------------
        // // http_get_echo                  Stable Success           100.0%   16.830302ms
        // // http_post_echo                 Stable Success           100.0%   12.270628ms
        // // http_put_echo                  Stable Success           100.0%   11.879437ms
        // // http_delete_echo               Stable Success           100.0%   13.389188ms
        // // http_patch_echo                Stable Success           100.0%   13.229072ms
        // // http_options_echo              Stable Success           100.0%   12.626277ms
        // // ----------------------------------------------------------------------
        // (http_get_echo),
        // (http_post_echo),
        // (http_put_echo),
        // (http_delete_echo),
        // (http_patch_echo),
        // (http_options_echo),
        //
        //
        // // 状态码
        // // ---------------------------- Function Details ---------------------------
        // // Function Name                  Classification        Success %  Median Time
        // // -------------------------------------------------------------------------------
        // // http_status_204                Stable Success           100.0%   10.291116ms
        // // http_status_304                Stable Success           100.0%   11.449748ms
        // // http_status_404                Stable Success           100.0%   10.234862ms
        // // http_status_500                Stable Success           100.0%   11.426395ms
        // // http_head_no_body              Stable Success           100.0%   10.373933ms
        // // http_redirect_chain            Stable Success           100.0%     60.0069ms
        // // http_multi_cookie              Stable Success           100.0%   12.348704ms
        // // ----------------------------------------------------------------------
        // (http_status_204),
        // (http_status_304),
        // (http_status_404),
        // (http_status_500),
        // (http_head_no_body),
        // (http_redirect_chain),
        // (http_multi_cookie),
        //
        //
        //
        // // 下载阶梯
        // // ---------------------------- Function Details ---------------------------
        // // Function Name                  Classification        Success %  Median Time
        // // -------------------------------------------------------------------------------
        // // dl_0b                          Stable Success           100.0%    10.98641ms
        // // dl_1b                          Stable Success           100.0%   10.660793ms
        // // dl_1kb                         Stable Success           100.0%   12.857031ms
        // // dl_64kb                        Stable Success           100.0%   30.532504ms
        // // dl_256kb                       Stable Success           100.0%   68.728044ms
        // // dl_1mb                         Stable Success           100.0%   237.76286ms
        // // dl_5mb                         Stable Success           100.0%  1.157749255s
        // // dl_10mb                        Stable Success           100.0%  2.289150769s
        // // dl_25mb                        Stable Success           100.0%  6.630321611s
        // // dl_30mb                        Stable Success           100.0%  6.997825806s
        // // dl_50mb                        Stable Success           100.0%  11.271576663s
        // (dl_0b),
        // (dl_1b),
        // (dl_1kb),
        // (dl_64kb),
        // (dl_256kb),
        // (dl_1mb),
        // (dl_5mb),
        // (dl_10mb),
        // (dl_25mb),
        // (dl_30mb, 15),
        // (dl_50mb, 20),
        //
        // // 上传阶梯
        // // ---------------------------- Function Details ---------------------------
        // // Function Name                  Classification        Success %  Median Time
        // // -------------------------------------------------------------------------------
        // // ul_0b                          Stable Success           100.0%    14.32951ms
        // // ul_1b                          Stable Success           100.0%   12.681413ms
        // // ul_1kb                         Stable Success           100.0%    12.38675ms
        // // ul_64kb                        Stable Success           100.0%   29.084599ms
        // // ul_256kb                       Stable Success           100.0%   71.804509ms
        // // ul_1mb                         Stable Success           100.0%   221.21929ms
        // // ul_5mb                         Stable Success           100.0%  993.138384ms
        // // ul_10mb                        Stable Success           100.0%  1.994511602s
        // // ul_25mb                        Stable Success           100.0%  4.913182856s
        // // ul_30mb                        Stable Success           100.0%  5.969460473s
        // // ul_50mb                        Stable Success           100.0%  9.803689804s
        // (ul_0b),
        // (ul_1b),
        // (ul_1kb),
        // (ul_64kb),
        // (ul_256kb),
        // (ul_1mb),
        // (ul_5mb),
        // (ul_10mb),
        // (ul_25mb),
        // (ul_30mb, 15),
        // (ul_50mb, 20),
        //
        // // 其他上传
        // // ---------------------------- Function Details ---------------------------
        // // Function Name                  Classification        Success %  Median Time
        // // -------------------------------------------------------------------------------
        // // up_zeros_64k                   Stable Success           100.0%   13.597323ms
        // // up_ff_64k                      Stable Success           100.0%   12.134797ms
        // // up_delim_pattern               Stable Success           100.0%   12.879354ms
        // // up_random_64k                  Stable Success           100.0%   40.218978ms
        // (up_zeros_64k),
        // (up_ff_64k),
        // (up_delim_pattern),
        // (up_random_64k),

        // chunked / 并发
        // ---------------------------- Function Details ---------------------------
        // Function Name                  Classification        Success %  Median Time
        // -------------------------------------------------------------------------------
        // concurrent_mixed_24            Stable Success           100.0%  889.144796ms
        // chunked_up_256kb               Stable Failure             0.0%   84.024694ms
        // ----------------------------------------------------------------------
        //
        // Issues Breakdown (Non-Successful Runs):
        //
        //   [STABLE FAILURE] chunked_up_256kb (Passed: 0/5)
        //     Run 1: FAILED (took 86.40ms)
        //       Reason: chunked size mismatch: 263941
        //     Run 2: FAILED (took 74.56ms)
        //       Reason: chunked size mismatch: 263941
        //     Run 3: FAILED (took 79.14ms)
        //       Reason: chunked size mismatch: 263941
        //     Run 4: FAILED (took 84.02ms)
        //       Reason: chunked size mismatch: 263941
        //     Run 5: FAILED (took 110.55ms)
        //       Reason: chunked size mismatch: 263941
        // ======================================================================
        // [RUNNING TEST]: chunked_up_256kb (5 runs, 10s timeout)
        // ----------------------------------------------------------------------
        // [wrangler:info] GET /health 200 OK (2ms)
        // [TEST_START] Start 1/5
        // [Client:debug] POST    localhost:18082 http://localhost:18082/upload Chunked
        // [wrangler:debug] [proxy http] start
        // [wrangler:debug] [proxy http] POST    localhost:18082 http://localhost:18082/upload
        // --> [localhost web] POST /upload
        // <-- [localhost web] POST /upload - 状态码: 200 OK (耗时: 19.585699ms)
        // [wrangler:info] POST /api/v3/auth 200 OK (40ms)
        // [TEST_FAILED]  Run 1/5 (took 86.40ms)
        // [wrangler:info] GET /health 200 OK (2ms)
        // [TEST_START] Start 2/5
        // [Client:debug] POST    localhost:18082 http://localhost:18082/upload Chunked
        // [wrangler:debug] [proxy http] start
        // [wrangler:debug] [proxy http] POST    localhost:18082 http://localhost:18082/upload
        // --> [localhost web] POST /upload
        // <-- [localhost web] POST /upload - 状态码: 200 OK (耗时: 18.52198ms)
        // [TEST_FAILED]  Run 2/5 (took 74.56ms)
        // [wrangler:info] POST /api/v3/auth 200 OK (26ms)
        // [wrangler:info] GET /health 200 OK (2ms)
        // [TEST_START] Start 3/5
        // [Client:debug] POST    localhost:18082 http://localhost:18082/upload Chunked
        // [wrangler:debug] [proxy http] start
        // [wrangler:debug] [proxy http] POST    localhost:18082 http://localhost:18082/upload
        // --> [localhost web] POST /upload
        // <-- [localhost web] POST /upload - 状态码: 200 OK (耗时: 19.446347ms)
        // [wrangler:info] POST /api/v3/auth 200 OK (28ms)
        // [TEST_FAILED]  Run 3/5 (took 79.14ms)
        // [wrangler:info] GET /health 200 OK (2ms)
        // [TEST_START] Start 4/5
        // [Client:debug] POST    localhost:18082 http://localhost:18082/upload Chunked
        // [wrangler:debug] [proxy http] start
        // [wrangler:debug] [proxy http] POST    localhost:18082 http://localhost:18082/upload
        // --> [localhost web] POST /upload
        // <-- [localhost web] POST /upload - 状态码: 200 OK (耗时: 21.326326ms)
        // [TEST_FAILED]  Run 4/5 (took 84.02ms)
        // [wrangler:info] POST /api/v3/auth 200 OK (32ms)
        // [wrangler:info] GET /health 200 OK (3ms)
        // [TEST_START] Start 5/5
        // [Client:debug] POST    localhost:18082 http://localhost:18082/upload Chunked
        // [wrangler:debug] [proxy http] start
        // [wrangler:debug] [proxy http] POST    localhost:18082 http://localhost:18082/upload
        // --> [localhost web] POST /upload
        // <-- [localhost web] POST /upload - 状态码: 200 OK (耗时: 24.468ms)
        // [TEST_FAILED]  Run 5/5 (took 110.55ms)
        // ======================================================================

        (chunked_up_256kb),
        (concurrent_mixed_24),


        // // 其他
        // // ---------------------------- Function Details ---------------------------
        // // Function Name                  Classification        Success %  Median Time
        // // -------------------------------------------------------------------------------
        // // negative_unconnectable_502     Stable Success           100.0%    11.86862ms
        // // gzip_body                      Stable Success           100.0%   17.926211ms
        // (negative_unconnectable_502),
        // (gzip_body),
    );

    print_report(results);

    Ok(())
}