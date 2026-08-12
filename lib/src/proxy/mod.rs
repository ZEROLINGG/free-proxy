// lib/src/proxy/mod.rs
// 客户端代理核心：
//   本地 HTTP 代理监听 → 接收完整请求 → 归一化 → 压缩+AEAD 加密 → POST 到 CF Worker
//   → 解析 SSE 流 → 解密解压 → 原样写回浏览器 socket（字节泵，无需解析响应）
//   CONNECT(TLS) 走 MITM：本地 CA 签发叶子证书，隧道内透传 HTTP/1.1 请求。
//   同一 TCP/TLS 连接上支持 HTTP Keep-Alive：只要客户端与“响应”双方都不
//   显式要求关闭，就在同一连接上继续处理下一个请求。
//

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::RwLock;
use std::sync::Arc;
use std::time::{SystemTime};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{timeout, timeout_at, Duration, Instant};

use crate::algo::{decode_chunk, encode_chunk};
use crate::base::{Base64, Encoder};
use crate::http::{http_parse_req, url_parse, HttpReqCache, UrlBuilder, MAX_HEADERS as HTTP_MAX_HEADERS};

mod sse;
mod tls;

pub use crate::tool::{gen_auth_token, xoroshiro128};
pub use tls::TlsManager;

/// 与 worker 建立连接的超时
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 等待浏览器发来第一个请求的超时（固定 deadline，不因为收到部分字节而被重置）
const FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// CONNECT 握手后等待隧道内首个请求的超时
const TLS_FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// 与浏览器完成 TLS(MITM) 握手允许的最长时间
const TLS_ACCEPT_TIMEOUT: Duration = Duration::from_secs(10);
/// Keep-Alive 场景下，等待同一连接上下一个请求到达的空闲超时
const KEEP_ALIVE_IDLE_TIMEOUT: Duration = Duration::from_secs(75);
/// 单个 SSE chunk 之间允许的最大间隔（超过视为假死连接）
const SSE_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 嗅探响应头以判断 Keep-Alive 意图时，最多缓存的字节数
const HEADER_SNIFF_LIMIT: usize = 1024 * 1024;
/// 单次读取缓冲大小
const READ_BUF: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub port: u16,                // 本地代理监听端口
    pub domain: String,
    /// 是否通过https连接worker 一般不推荐
    pub use_https: bool,
    pub auth_key: String,
    pub ca_dir: PathBuf,
    pub compressor: ProxyCompressor,
    pub aead: ProxyAead,
    /// 可选的优选 IP
    pub pref_ip: Option<String>,
}

/// 算法类型与 URL 路径契约的单一实现位于 `crate::algo`，
/// 客户端/服务端共用，此处仅重新导出保持兼容。
pub use crate::algo::{ProxyAead, ProxyAlgo, ProxyCompressor};
use crate::tool::derive_keys;

struct Shared {
    worker_url: String,
    token_base: [u8; 16],
    key16: [u8; 16],
    key32: [u8; 32],
    algo: RwLock<ProxyAlgo>,
    client: reqwest::Client,
    pref_ip: RwLock<Option<String>>,
    pref_client: RwLock<Option<reqwest::Client>>,
    tls: Arc<TlsManager>,
}




pub struct Proxy {
    cfg: ProxyConfig,
    shared: Arc<Shared>,
    task: Option<JoinHandle<Result<()>>>,
}

impl Proxy {
    pub fn new(mut cfg: ProxyConfig) -> Result<Self> {
        anyhow::ensure!(!cfg.auth_key.is_empty(), "auth_key must not be empty");
        cfg.domain = cfg.domain.trim().to_string();

        let worker_url = UrlBuilder::new()
            .https(cfg.use_https)
            .host(cfg.domain.as_str())
            .build()?;

        let derived = derive_keys(&cfg.auth_key, &cfg.domain)?;
        let (key16, key32, token_base) = (derived.key16, derived.key32, derived.token_base);

        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .no_proxy()
            .pool_max_idle_per_host(512)
            .pool_idle_timeout(Duration::from_secs(360))
            .tcp_keepalive(Duration::from_secs(60))
            .http2_initial_stream_window_size(2 * 1024 * 1024)
            .http2_initial_connection_window_size(16 * 1024 * 1024)
            .tcp_nodelay(true)
            .build()?;
        let tls = Arc::new(TlsManager::init(&cfg.ca_dir)?);

        let initial_algo = ProxyAlgo::new(cfg.compressor, cfg.aead);

        // 初始化优选IP相关的Client
        let pref_ip_str = cfg
            .pref_ip
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let initial_pref_ip = pref_ip_str.map(str::to_owned);
        let pref_client = build_pref_client(&worker_url, pref_ip_str)?;

        Ok(Self {
            cfg,
            shared: Arc::new(Shared {
                worker_url,
                token_base,
                key16,
                key32,
                algo: RwLock::new(initial_algo),
                client,
                pref_ip: RwLock::new(initial_pref_ip),
                pref_client: RwLock::new(pref_client),
                tls,
            }),
            task: None,
        })
    }

    /// 启动本地 HTTP 代理（仅监听 127.0.0.1），返回实际端口
    pub async fn start(&mut self) -> Result<u16> {
        if self.task.is_some() {
            return Err(anyhow!("proxy already started"));
        }

        let listener = TcpListener::bind(("127.0.0.1", self.cfg.port))
            .await
            .with_context(|| format!("failed to bind 127.0.0.1:{}", self.cfg.port))?;
        let port = listener.local_addr()?.port();
        let shared = Arc::clone(&self.shared);

        let task = tokio::spawn(async move {
            loop {
                let (socket, addr) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("proxy: accept error: {e}");
                        break;
                    }
                };
                let shared = Arc::clone(&shared);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(socket, shared).await {
                        if is_benign_disconnect(&e) {
                            #[cfg(debug_assertions)]
                            eprintln!("proxy: connection {addr}: {e:#} (benign disconnect)");
                        } else {
                            eprintln!("proxy: connection {addr}: {e:#?}");
                        }
                    }
                });
            }
            Ok(())
        });

        self.task = Some(task);
        Ok(port)
    }
    pub async fn check_availability(&self) -> Result<ProxyCheck> {
        if !self.is_running() {
            bail!("proxy is not running (call start() first)");
        }
        check_proxy_availability(self.cfg.port).await
    }

    /// 停止代理（中止监听循环，不等待）
    pub async fn stop(&mut self) {
        if let Some(handle) = self.task.take() {
            handle.abort();
            let _ = handle.await;
        }
    }

    pub fn port(&self) -> u16 {
        self.cfg.port
    }

    pub fn is_running(&self) -> bool {
        self.task.is_some()
    }

    pub fn ca_cert_path(&self) -> PathBuf {
        TlsManager::ca_cert_path(&self.cfg.ca_dir)
    }

    /// 当前启用的算法组合
    pub fn algo(&self) -> ProxyAlgo {
        *self
            .shared
            .algo
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 热切换压缩/加密算法：对后续请求立即生效，无需重启代理
    pub fn set_algo(&self, compressor: ProxyCompressor, aead: ProxyAead) {
        let mut guard = self
            .shared
            .algo
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = ProxyAlgo::new(compressor, aead);
    }

    /// 热切换 AEAD 加密算法（接受字符串，如 "aes128gcm" / "chacha20poly1305"）
    pub fn set_aead(&self, aead: &str) -> Result<()> {
        let aead: ProxyAead = aead
            .parse()
            .with_context(|| format!("invalid aead: {aead:?}"))?;

        let mut guard = self
            .shared
            .algo
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.aead = aead;
        Ok(())
    }

    /// 热切换压缩算法（接受字符串，如 "zstd" / "gzip" / "lz4"）
    pub fn set_compressor(&self, compressor: &str) -> Result<()> {
        let compressor: ProxyCompressor = compressor
            .parse()
            .with_context(|| format!("invalid compressor: {compressor:?}"))?;

        let mut guard = self
            .shared
            .algo
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.compressor = compressor;
        Ok(())
    }

    /// 当前优选 IP（None 表示走 DNS 解析）
    pub fn ip(&self) -> Option<String> {
        let guard = self
            .shared
            .pref_ip
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    }

    /// 热切换优选 IP：Some(ip) 时构建带 `.resolve(domain, ip:port)` 的专用
    /// reqwest client（SNI/Host 仍是域名）；None 清除并回退 DNS 解析。
    pub fn set_ip(&self, ip: Option<&str>) -> Result<()> {
        let ip = ip.map(str::trim).filter(|s| !s.is_empty());
        let client = build_pref_client(&self.shared.worker_url, ip)?;

        let mut ip_guard = self
            .shared
            .pref_ip
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut client_guard = self
            .shared
            .pref_client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *ip_guard = ip.map(str::to_owned);
        *client_guard = client;
        Ok(())
    }
}

/// 代理可用性检测结果
#[derive(Debug, Clone)]
pub struct ProxyCheck {
    pub ip: String,
    pub latency_ms: u64,
}

/// 通过本地代理请求 http://ip.me，返回出口 IP 与整链路耗时（最长 10s）。
pub async fn check_proxy_availability(port: u16) -> Result<ProxyCheck> {
    let proxy_url = UrlBuilder::new()
        .https(false)
        .host("127.0.0.1")
        .port(port)
        .build()
        .context("failed to configure proxy setting")?;
    let proxy = reqwest::Proxy::all(&proxy_url).context("failed to configure proxy setting")?;

    let check_client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
        .context("failed to build check client")?;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis();

    let start = Instant::now();

    let mut set = JoinSet::new();
    let endpoints = ["http://api.ipify.org", "http://ip.me"];

    // 发起6个并发请求（每个接口各3个）
    for i in 0..6 {
        let client = check_client.clone();
        let endpoint = endpoints[i % 2];
        let query = format!("{now}_{i}");
        let url = UrlBuilder::new()
            .base(endpoint)
            .append_query("_t", &query)
            .build()
            .with_context(|| format!("failed to build url for {endpoint}"))?;

        set.spawn(async move {
            let resp = client
                .get(&url)
                .send()
                .await
                .with_context(|| format!("failed to request {url} via local proxy"))?;

            if !resp.status().is_success() {
                bail!("{} returned error status: {}", url, resp.status());
            }

            let ip = resp.text().await.context("failed to read response body")?;
            let clean_ip = ip.trim().to_string();

            if clean_ip.is_empty() {
                bail!("received empty response from {}", url);
            }

            Ok::<ProxyCheck, anyhow::Error>(ProxyCheck {
                ip: clean_ip,
                latency_ms: start.elapsed().as_millis() as u64,
            })
        });
    }

    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(proxy_check)) => {
                set.abort_all();
                return Ok(proxy_check);
            }
            Ok(Err(_)) => continue,
            Err(_) => continue,
        }
    }

    bail!("All 6 concurrent proxy checks failed")
}

struct ParsedRequest {
    /// 原始请求字节，原样转发给 worker
    raw: Vec<u8>,
    method: String,
    /// origin-form 下是路径；CONNECT 下是 "host:port" authority
    path: String,
    keep_alive: bool,
}

impl ParsedRequest {
    /// 解析一段完整的请求。full_url 仅在服务端需要时经
    /// `HttpReq::full_url(protocol)` 按需构建，客户端不构造。
    fn parse(raw: Vec<u8>) -> Result<Self> {
        let req = http_parse_req(&raw).context("failed to parse request")?;

        let method = req.method.to_string();
        let path = req.path.to_string();
        let keep_alive = connection_keep_alive(
            req.version,
            req.headers
                .iter()
                .map(|(name, value)| (*name, value.as_bytes())),
        );
        println!("method: {method}, url: {path} keep_alive: {keep_alive}");

        Ok(Self {
            raw,
            method,
            path,
            keep_alive,
        })
    }

    fn is_connect(&self) -> bool {
        self.method.eq_ignore_ascii_case("CONNECT")
    }
}

// ─── 连接处理 ─────────────────────────────────────────────────────────────────

/// 从 stream 中读取下一个请求的结果
enum ReadOutcome {
    Request(Vec<u8>),
    /// 对端在完整请求到达前正常关闭了连接（EOF，含 TLS 无 close_notify 的场景）
    Closed,
    /// 直到 deadline 都没有等到完整请求
    TimedOut,
}

/// 从 stream 中持续读取数据，直到从 cache 中弹出一个完整请求、
/// 对端正常关闭连接，或者到达 deadline。
async fn read_request<S>(
    stream: &mut S,
    cache: &mut HttpReqCache,
    deadline: Instant,
) -> Result<ReadOutcome>
where
    S: AsyncRead + Unpin,
{
    if let Some(req) = cache.pop()? {
        return Ok(ReadOutcome::Request(req));
    }

    loop {
        let mut buf = [0u8; READ_BUF];
        let n = match timeout_at(deadline, stream.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(ReadOutcome::Closed);
            }
            Ok(Err(e)) => return Err(e).context("read failed"),
            Err(_) => return Ok(ReadOutcome::TimedOut),
        };
        if n == 0 {
            return Ok(ReadOutcome::Closed);
        }
        cache.push(&buf[..n])?;
        if let Some(req) = cache.pop()? {
            return Ok(ReadOutcome::Request(req));
        }
    }
}

async fn handle_connection(mut socket: TcpStream, shared: Arc<Shared>) -> Result<()> {
    let mut cache = HttpReqCache::new();
    let deadline = Instant::now() + FIRST_REQUEST_TIMEOUT;

    let first_raw = match read_request(&mut socket, &mut cache, deadline).await? {
        ReadOutcome::Request(r) => r,
        ReadOutcome::Closed => return Ok(()), // 浏览器提前断开
        ReadOutcome::TimedOut => bail!("timed out waiting for first request"),
    };

    let parsed = ParsedRequest::parse(first_raw)?;

    if parsed.is_connect() {
        handle_connect(socket, parsed, shared).await
    } else {
        serve_http(socket, cache, parsed, false, shared).await
    }
}

/// CONNECT：在本地完成 TLS 握手（MITM），隧道内解析并转发 HTTP/1.1 请求。
/// 隧道内同样支持 Keep-Alive（同一 TLS 连接上串行处理多个 HTTPS 请求）。
async fn handle_connect(
    mut socket: TcpStream,
    connect_req: ParsedRequest,
    shared: Arc<Shared>,
) -> Result<()> {
    let authority = connect_req.path.trim().to_string();
    if authority.is_empty() {
        bail!("CONNECT without authority");
    }
    let host = authority
        .split(':')
        .next()
        .unwrap_or(&authority)
        .to_string();
    if host.is_empty() {
        bail!("CONNECT without authority");
    }

    socket
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    socket.flush().await?;

    let mut tls_stream = timeout(TLS_ACCEPT_TIMEOUT, shared.tls.accept(socket, Some(&host)))
        .await
        .context("timed out establishing TLS with client")?
        .context("TLS handshake failed")?;

    let mut cache = HttpReqCache::new();
    let deadline = Instant::now() + TLS_FIRST_REQUEST_TIMEOUT;
    let first_raw = match read_request(&mut tls_stream, &mut cache, deadline).await? {
        ReadOutcome::Request(r) => r,
        ReadOutcome::Closed => return Ok(()),
        ReadOutcome::TimedOut => bail!("timed out waiting for tunneled request"),
    };

    let parsed = ParsedRequest::parse(first_raw)?;
    serve_http(tls_stream, cache, parsed, true, shared).await
}

/// 支持 Keep-Alive 的请求处理循环：在同一连接上串行处理多个请求，
/// 直到请求方或响应方任一方显式要求关闭、发生错误、达到请求数上限，
/// 或空闲超时。
async fn serve_http<S>(
    mut stream: S,
    mut cache: HttpReqCache,
    mut parsed: ParsedRequest,
    is_https: bool,
    shared: Arc<Shared>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{

    let result: Result<()> = loop {


        let want_keep_alive = parsed.keep_alive;

        let keep_alive = match handle_request(
            &mut stream,
            &mut cache,
            &parsed.raw,
            is_https,
            &shared,
            want_keep_alive,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => break Err(e),
        };

        if !keep_alive {
            break Ok(());
        }

        let deadline = Instant::now() + KEEP_ALIVE_IDLE_TIMEOUT;
        match read_request(&mut stream, &mut cache, deadline).await {
            Ok(ReadOutcome::Request(r)) => match ParsedRequest::parse(r) {
                Ok(p) => parsed = p,
                Err(e) => break Err(e),
            },
            Ok(ReadOutcome::Closed) | Ok(ReadOutcome::TimedOut) => break Ok(()),
            Err(e) => break Err(e),
        }
    };

    let _ = stream.shutdown().await;
    result
}

/// 转发单个请求：打包 → POST worker → SSE 解包 → 字节写回浏览器。
/// 返回值表示这条连接是否应当继续保持（供下一个请求复用）。
///
/// `cache` 与 `serve_http` 中用于读取“下一个请求”的缓冲区是同一个：
/// 转发响应期间顺带探测浏览器是否已断开时（`browser_closed`），任何提前
/// 到达的字节都会被追加进这个 `cache`，而不会丢失。
async fn handle_request<S>(
    stream: &mut S,
    cache: &mut HttpReqCache,
    req_bytes: &[u8],
    is_https: bool,
    shared: &Arc<Shared>,
    want_keep_alive: bool,
) -> Result<bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut payload = req_bytes.to_vec();
    payload.push(if is_https { 1 } else { 0 });

    // 每次请求读取当前算法（支持热切换）；加密/解密必须用同一组合
    let algo = *shared
        .algo
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // 与服务端对称：先压缩后加密（共享管线，见 crate::algo）
    let body = encode_chunk(
        &payload,
        algo.compressor,
        algo.aead,
        &shared.key16,
        &shared.key32,
    )?;

    // 优选 IP：设置了则用带 resolve 的专用 client，否则回退 DNS
    let pref_client = shared
        .pref_client
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();

    let url = UrlBuilder::new()
        .base(shared.worker_url.as_str())
        .path(algo.api_path().as_str())
        .build()
        .context("build worker request url failed")?;

    let resp = pref_client
        .as_ref()
        .unwrap_or(&shared.client)
        .post(url)
        .bearer_auth(gen_auth_token(&shared.token_base))
        .header("Content-Type", "application/octet-stream")
        .body(body)
        .send()
        .await
        .context("worker request failed")?;

    if !resp.status().is_success() {
        println!(
            "worker request error: {}, body: {:.512}...",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        write_502(stream).await?;
        return Ok(false);
    }

    let mut parser = sse::SseParser::new();
    let mut sse_stream = Box::pin(resp.bytes_stream());
    let mut wrote = false;

    let mut header_buf: Vec<u8> = Vec::new();
    let mut response_wants_keep_alive: Option<bool> = None;

    loop {
        tokio::select! {
            chunk = timeout(SSE_IDLE_TIMEOUT, sse_stream.next()) => {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(_) => {
                        eprintln!("proxy: sse idle timeout (no data for {:?})", SSE_IDLE_TIMEOUT);
                        if !wrote {
                            write_502(stream).await?;
                        }
                        return Ok(false);
                    }
                };

                let bytes = match chunk {
                    Some(Ok(b)) => b,
                    Some(Err(e)) => {
                        eprintln!("proxy: sse read error: {e}");
                        return Ok(false);
                    }
                    None => return Ok(false), // 流提前结束（无 done）
                };

                let mut events = Vec::new();
                parser.push(&bytes, &mut events);

                for ev in events {
                    match ev {
                        sse::SseEvent::Data(b64) => {
                            let enc = Base64::decode(&b64).context("bad base64 payload")?;
                            let raw = decode_chunk(
                                &enc,
                                algo.compressor,
                                algo.aead,
                                &shared.key16,
                                &shared.key32,
                            )
                            .context("decrypt/decompress failed")?;


                            if response_wants_keep_alive.is_none()
                                && header_buf.len() < HEADER_SNIFF_LIMIT
                            {
                                let remaining = HEADER_SNIFF_LIMIT - header_buf.len();
                                let take = remaining.min(raw.len());
                                header_buf.extend_from_slice(&raw[..take]);
                                if let Some(ka) = response_keep_alive(&header_buf) {
                                    response_wants_keep_alive = Some(ka);
                                } else if header_buf.len() >= HEADER_SNIFF_LIMIT {
                                    // 头部异常巨大或格式无法识别，保守按 close 处理
                                    response_wants_keep_alive = Some(false);
                                }
                            }

                            stream.write_all(&raw).await?;
                            // println!("{:.1024}...", format!("{:?}", String::from_utf8_lossy(&raw)));
                            wrote = true;
                        }
                        sse::SseEvent::Error(msg) => {
                            if !wrote {
                                write_502(stream).await?;
                            }
                            eprintln!("proxy: worker error: {msg}");
                            return Ok(false);
                        }
                        sse::SseEvent::Done => {
                            stream.flush().await?;
                            let response_ka = response_wants_keep_alive.unwrap_or(false);
                            return Ok(want_keep_alive && response_ka);
                        }
                    }
                }
            }
            closed = browser_closed(stream, cache) => {
                if closed? {
                    return Ok(false); // 浏览器已断开，丢弃残余工作
                }
            }
        }
    }
}

/// 探测浏览器是否已断开（读取可用字节）。
async fn browser_closed<S: AsyncRead + Unpin>(
    stream: &mut S,
    cache: &mut HttpReqCache,
) -> Result<bool> {
    let mut buf = [0u8; READ_BUF];
    match stream.read(&mut buf).await {
        Ok(0) | Err(_) => Ok(true),
        Ok(n) => {
            cache
                .push(&buf[..n])
                .context("browser_closed: cache push failed")?;
            Ok(false)
        }
    }
}

async fn write_502<S: AsyncWrite + Unpin>(stream: &mut S) -> Result<()> {
    stream
        .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await?;
    stream.flush().await?;
    Ok(())
}

// ─── Keep-Alive 判定 ──────────────────────────────────────────────────────────

/// 依据（部分）响应头字节判断服务端是否希望保持连接。
/// 返回 `None` 表示 header 尚未收全（或解析失败但尚未超出嗅探上限），
/// 需要更多数据。
fn response_keep_alive(data: &[u8]) -> Option<bool> {
    let mut headers = [httparse::EMPTY_HEADER; HTTP_MAX_HEADERS];
    let mut resp = httparse::Response::new(&mut headers);
    match resp.parse(data) {
        Ok(httparse::Status::Complete(_)) => {
            let version = resp.version.unwrap_or(0);
            Some(connection_keep_alive(
                version,
                resp.headers.iter().map(|h| (h.name, h.value)),
            ))
        }
        _ => None,
    }
}

/// HTTP/1.1 默认 keep-alive，HTTP/1.0 默认 close；
/// 显式的 Connection 头（可能是逗号分隔的多个 token）优先生效。
fn connection_keep_alive<'a>(
    version: u8,
    headers: impl Iterator<Item = (&'a str, &'a [u8])>,
) -> bool {
    let mut keep_alive = version >= 1;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("connection") {
            let val = String::from_utf8_lossy(value).to_ascii_lowercase();
            for token in val.split(',') {
                match token.trim() {
                    "close" => keep_alive = false,
                    "keep-alive" => keep_alive = true,
                    _ => {}
                }
            }
        }
    }
    keep_alive
}

// ─── 错误分类（日志降噪） ─────────────────────────────────────────────────────

/// 判断一个连接处理错误是否属于“客户端主动/异常断开”这类正常网络噪音
/// （而非代理自身逻辑故障），用于避免此类高频事件把真正的错误日志淹没。
fn is_benign_disconnect(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .map(|io_err| {
                matches!(
                    io_err.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::UnexpectedEof
                )
            })
            .unwrap_or(false)
    })
}

// ─── 小工具 ───────────────────────────────────────────────────────────────────

/// 构建优选 IP 专用 reqwest client（`.resolve(host, ip:port)`，SNI/Host 仍是域名）。
/// 传入空串/None 返回 `None`，表示走正常 DNS 解析。
fn build_pref_client(worker_url: &str, ip: Option<&str>) -> Result<Option<reqwest::Client>> {
    let Some(ip) = ip.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let url = url_parse(worker_url)?;
    let host = url.host.ok_or_else(|| anyhow!("invalid worker url: {worker_url:?}"))?;
    let port = url.port.ok_or_else(|| anyhow!("invalid worker url: {worker_url:?}"))?;
    let addr: IpAddr = ip
        .parse()
        .with_context(|| format!("invalid preferred ip: {ip:?}"))?;
    let client = reqwest::Client::builder()
        .resolve(&*host, SocketAddr::new(addr, port))
        .connect_timeout(CONNECT_TIMEOUT)
        .no_proxy()
        .pool_max_idle_per_host(512)
        .pool_idle_timeout(Duration::from_secs(360))
        .tcp_keepalive(Duration::from_secs(60))
        .http2_initial_stream_window_size(2 * 1024 * 1024)
        .http2_initial_connection_window_size(16 * 1024 * 1024)
        .tcp_nodelay(true)
        .build()?;
    Ok(Some(client))
}

