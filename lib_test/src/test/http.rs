// lib_test/src/test/http.rs
//
// 明文 HTTP 代理场景用例矩阵（非 WS）。
// 每个用例经 BROWSER（reqwest，全量走本地代理）→ 客户端隧道帧化加密
// → worker 重构请求 → 本地目标站(web.rs)，覆盖：
//   方法透传 / 状态码与无 body 特殊路径 / 大小阶梯上下行 /
//   二进制对抗样本 / chunked 上传 / keep-alive / 并发 / 慢速流 / 负向 502。

use anyhow::{Result, ensure};
use reqwest::Method;
use tokio::task::JoinSet;

use crate::test::BROWSER;
use crate::util;

/// 本地目标站基址
fn base() -> &'static str {
    crate::web::baseurl()
}

// ─── 公共检查 ────────────────────────────────────────────────────────────────

async fn download_check(size: u64) -> Result<()> {
    let url = format!("{}/download/{size}", base());
    let resp = BROWSER.get(&url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    ensure!(
        bytes.len() as u64 == size,
        "size mismatch: got {}, want {size}",
        bytes.len()
    );
    let mut expect = vec![0u8; size as usize];
    util::fill_pattern(&mut expect, 0);
    ensure!(
        bytes.as_ref() == expect.as_slice(),
        "content mismatch at size {size}"
    );
    Ok(())
}

async fn upload_check(size: usize) -> Result<()> {
    let data = util::pattern_bytes(size);
    let expect_len = data.len() as u64;
    let expect_hash = util::blake3_hex(&data);

    let url = format!("{}/upload", base());
    let resp = BROWSER
        .post(&url)
        .body(data)
        .send()
        .await?
        .error_for_status()?;
    let (len, hash) = util::parse_len_blake3(&resp.text().await?)?;
    ensure!(
        len == expect_len,
        "size echo mismatch: server {len} != sent {expect_len}"
    );
    ensure!(
        hash == expect_hash,
        "blake3 mismatch: server={hash} local={expect_hash}"
    );
    Ok(())
}

/// 大小阶梯：下载 + 上传对称生成用例函数
macro_rules! ladder_cases {
    ($(($dl:ident, $ul:ident, $size:expr)),+ $(,)?) => {
        $(
            pub async fn $dl() -> Result<()> { download_check($size).await }
            pub async fn $ul() -> Result<()> { upload_check($size).await }
        )+
    };
}

ladder_cases! {
    (dl_0b,    ul_0b,    0),
    (dl_1b,    ul_1b,    1),
    (dl_1kb,   ul_1kb,   1024),
    (dl_64kb,  ul_64kb,  64 * 1024),
    (dl_256kb, ul_256kb, 256 * 1024),
    (dl_1mb,   ul_1mb,   1024 * 1024),
    (dl_5mb,   ul_5mb,   5 * 1024 * 1024),
    (dl_10mb,  ul_10mb,  10 * 1024 * 1024),
    (dl_25mb,  ul_25mb,  25 * 1024 * 1024),
    (dl_30mb,  ul_30mb,  30 * 1024 * 1024),
    (dl_50mb,  ul_50mb,  50 * 1024 * 1024),
}

// ─── 方法透传 ────────────────────────────────────────────────────────────────

async fn method_echo(m: Method) -> Result<String> {
    let url = format!("{}/echo", base());
    let resp = BROWSER
        .request(m.clone(), &url)
        .body("x")
        .send()
        .await?
        .error_for_status()?;
    let text = resp.text().await?;
    ensure!(
        text.contains(&format!("method={m}")),
        "method not echoed: {}",
        text.lines().next().unwrap_or("")
    );
    Ok(text)
}

pub async fn http_get_echo() -> Result<()> {
    method_echo(Method::GET).await.map(|_| ())
}
pub async fn http_post_echo() -> Result<()> {
    method_echo(Method::POST).await.map(|_| ())
}
pub async fn http_put_echo() -> Result<()> {
    method_echo(Method::PUT).await.map(|_| ())
}
pub async fn http_delete_echo() -> Result<()> {
    method_echo(Method::DELETE).await.map(|_| ())
}
pub async fn http_patch_echo() -> Result<()> {
    method_echo(Method::PATCH).await.map(|_| ())
}
pub async fn http_options_echo() -> Result<()> {
    method_echo(Method::OPTIONS).await.map(|_| ())
}

// ─── 状态码与特殊路径 ────────────────────────────────────────────────────────

pub async fn http_status_204() -> Result<()> {
    let resp = BROWSER.get(format!("{}/status/204", base())).send().await?;
    ensure!(resp.status() == 204, "want 204, got {}", resp.status());
    ensure!(resp.bytes().await?.is_empty(), "204 must have empty body");
    Ok(())
}

pub async fn http_status_304() -> Result<()> {
    let resp = BROWSER.get(format!("{}/status/304", base())).send().await?;
    ensure!(resp.status() == 304, "want 304, got {}", resp.status());
    ensure!(resp.bytes().await?.is_empty(), "304 must have empty body");
    Ok(())
}

pub async fn http_status_404() -> Result<()> {
    let resp = BROWSER.get(format!("{}/status/404", base())).send().await?;
    ensure!(
        resp.status() == 404,
        "want 404 passthrough, got {}",
        resp.status()
    );
    ensure!(
        resp.text().await?.contains("status-body-404"),
        "404 body lost"
    );
    Ok(())
}

pub async fn http_status_500() -> Result<()> {
    let resp = BROWSER.get(format!("{}/status/500", base())).send().await?;
    ensure!(
        resp.status() == 500,
        "want 500 passthrough, got {}",
        resp.status()
    );
    ensure!(
        resp.text().await?.contains("status-body-500"),
        "500 body lost"
    );
    Ok(())
}

/// HEAD：worker 响应侧走"无 body、无 chunked tail"分支
pub async fn http_head_no_body() -> Result<()> {
    let resp = BROWSER
        .head(format!("{}/download/1024", base()))
        .send()
        .await?;
    ensure!(resp.status() == 200, "want 200, got {}", resp.status());
    ensure!(
        resp.bytes().await?.is_empty(),
        "HEAD response must be empty"
    );
    Ok(())
}

/// 多跳重定向：worker manual-redirect 原样回传 3xx，由客户端跟随
pub async fn http_redirect_chain() -> Result<()> {
    let resp = BROWSER
        .get(format!("{}/redirect/3", base()))
        .send()
        .await?
        .error_for_status()?;
    ensure!(
        resp.url().path() == "/redirect-done",
        "unexpected final url: {}",
        resp.url()
    );
    ensure!(resp.text().await? == "redirect-done", "final body lost");
    Ok(())
}

/// 多个 Set-Cookie 头原样保留
pub async fn http_multi_cookie() -> Result<()> {
    let resp = BROWSER.get(format!("{}/cookies", base())).send().await?;
    let n = resp.headers().get_all("set-cookie").iter().count();
    ensure!(n == 3, "want 3 set-cookie headers, got {n}");
    Ok(())
}

// ─── 二进制对抗样本上传 ──────────────────────────────────────────────────────

async fn upload_raw(payload: Vec<u8>) -> Result<()> {
    let expect_len = payload.len() as u64;
    let expect_hash = util::blake3_hex(&payload);
    let url = format!("{}/upload", base());
    let resp = BROWSER
        .post(&url)
        .body(payload)
        .send()
        .await?
        .error_for_status()?;
    let (len, hash) = util::parse_len_blake3(&resp.text().await?)?;
    ensure!(
        len == expect_len,
        "size echo mismatch: {len} != {expect_len}"
    );
    ensure!(
        hash == expect_hash,
        "blake3 mismatch: server={hash} local={expect_hash}"
    );
    Ok(())
}

/// 全零：压缩率极端
pub async fn up_zeros_64k() -> Result<()> {
    upload_raw(vec![0u8; 64 * 1024]).await
}

/// 全 0xFF
pub async fn up_ff_64k() -> Result<()> {
    upload_raw(vec![0xFF; 64 * 1024]).await
}

/// 重复 "0\r\n\r\n" 序列：打击任何按字节流做边界猜测的实现
pub async fn up_delim_pattern() -> Result<()> {
    upload_raw(b"0\r\n\r\n".repeat(13_107)).await
}

/// 纯随机不可压缩数据
pub async fn up_random_64k() -> Result<()> {
    let payload: Vec<u8> = (0..64 * 1024).map(|_| rand::random::<u8>()).collect();
    upload_raw(payload).await
}

// ─── chunked 上传（Transfer-Encoding: chunked 全链路）───────────────────────

/// 256 个 1KB 块以未知长度流发送，强制 chunked 分帧，
/// 打通 PumpTracker::Chunked + ChunkedEndScanner 四态状态机的 E2E 路径
pub async fn chunked_up_256kb() -> Result<()> {
    let total_chunks = 256u32;
    let mut expect_hasher_input = Vec::with_capacity(total_chunks as usize * 1024);

    let chunks: Vec<Result<Vec<u8>, std::io::Error>> = (0..total_chunks)
        .map(|i| {
            let mut buf = vec![0u8; 1024];
            util::fill_pattern(&mut buf, i as u64 * 1024);
            expect_hasher_input.extend_from_slice(&buf);
            Ok(buf)
        })
        .collect();
    let expect_hash = util::blake3_hex(&expect_hasher_input);

    let stream = futures_util::stream::iter(chunks);
    let url = format!("{}/upload", base());
    let resp = BROWSER
        .post(&url)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await?
        .error_for_status()?;
    let (len, hash) = util::parse_len_blake3(&resp.text().await?)?;
    ensure!(
        len == expect_hasher_input.len() as u64,
        "chunked size mismatch: {len}"
    );
    ensure!(
        hash == expect_hash,
        "chunked blake3 mismatch: server={hash} local={expect_hash}"
    );
    Ok(())
}


/// 并发混合下载：多路复用同一代理端口
pub async fn concurrent_mixed_24() -> Result<()> {
    let sizes = [
        1024u64,
        64 * 1024,
        256 * 1024,
        1024,
        64 * 1024,
        256 * 1024,
        1024 * 1024,
        1024,
        1024u64,
        64 * 1024,
        256 * 1024,
        1024,
        64 * 1024,
        256 * 1024,
        1024 * 1024,
        1024,
        1024u64,
        64 * 1024,
        256 * 1024,
        1024,
        64 * 1024,
        256 * 1024,
        1024 * 1024,
        1024,
    ];
    let mut set = JoinSet::new();
    for size in sizes {
        set.spawn(async move { download_check(size).await });
    }
    let mut failed = 0usize;
    while let Some(res) = set.join_next().await {
        if res
            .map_err(|e| anyhow::anyhow!("join error: {e}"))?
            .is_err()
        {
            failed += 1;
        }
    }
    ensure!(
        failed == 0,
        "{failed}/{} concurrent downloads failed",
        sizes.len()
    );
    Ok(())
}


/// 必然连接失败的目标：worker fetch 报错 → 客户端应收到合成 502
pub async fn negative_unconnectable_502() -> Result<()> {
    let resp = BROWSER.get("http://127.0.0.1:9/").send().await?;
    ensure!(
        resp.status() == 502,
        "want synthesized 502, got {}",
        resp.status()
    );
    Ok(())
}


pub async fn gzip_body() -> Result<()> {
    let resp = BROWSER
        .get(format!("{}/gzip", base()))
        .send()
        .await?
        .error_for_status()?;
    let ce = resp
        .headers()
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = resp.bytes().await?;

    // worker已经自动解压
    let mut expect = vec![0x1f, 0x8b];
    expect.extend_from_slice(b"GZIP-MARKER-PAYLOAD");
    ensure!(
        body.as_ref() == expect.as_slice(),
        "gzip route body corrupted en route"
    );
    Ok(())
}
