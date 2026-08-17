// lib/src/proxy/mod.rs
//
// 本地 HTTP 代理核心（MITM TLS 在 tls.rs）：
//   - 每个请求头只跑一次 httparse（HeaderPaser），method/path/raw 零拷贝复用；
//   - 请求体按 HTTP 语义判定范围（Content-Length 计数 / chunked 四态扫描 /
//     无 body），读完浏览器请求体立即以 EOS 完成 worker 请求体，再等待响应；
//     （Cloudflare edge 在请求体未完成前不会交付响应，"等响应再 EOS"会死锁）
//   - 明文 HTTP / CONNECT 隧道内 HTTPS 统一为泛型 serve() 循环（编译期单态化两份）；
//   - keep-alive：收到 EOS 且客户端未断开即可复用连接。

use anyhow::{anyhow, bail, Context, Result};
use bytes::{Buf, Bytes, BytesMut};
use futures_util::StreamExt;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::SystemTime;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{timeout, timeout_at, Duration, Instant};

use crate::algo::{decode_chunk, encode_chunk};
use crate::frames::{make_frame, Frame, FrameCache};
use crate::http::{split_host_port, url_parse, HeaderPaser, ReqHeader, UrlBuilder};

mod tls;

pub use crate::tool::{gen_auth_token, xoroshiro128};
pub use tls::TlsManager;

/// 与 worker 建立连接的超时
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// 等待浏览器发来第一个请求的超时（固定 deadline，不因为收到部分字节而被重置）
const FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(96);
/// CONNECT 握手后等待隧道内首个请求的超时
const TLS_FIRST_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// 与浏览器完成 TLS(MITM) 握手允许的最长时间
const TLS_ACCEPT_TIMEOUT: Duration = Duration::from_secs(10);
/// Keep-Alive 场景下，等待同一连接上下一个请求到达的空闲超时
const KEEP_ALIVE_IDLE_TIMEOUT: Duration = Duration::from_secs(96);
/// 单个帧之间允许的最大间隔（超过视为假死连接）
const FRAME_IDLE_TIMEOUT: Duration = Duration::from_secs(96);
/// 请求 body 泵送阶段：两次成功读取之间的最大间隔（上传卡死兜底）
const BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(96);

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
    /// CA 私钥保护密钥（32B，由设备 uid + 随机盐派生，见 derive_ca_key_secret）
    pub ca_key_secret: [u8; 32],
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

        // domain 不允许携带端口：worker 侧密钥派生与 token 校验均基于纯 host（env secret
        // "domain"），带端口会导致两端 token 不匹配（全链路 401）。
        let (_host, port) = split_host_port(&cfg.domain).context("invalid domain")?;
        anyhow::ensure!(
            port.is_none(),
            "domain must not contain a port, got {:?}",
            cfg.domain
        );

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
        let tls = Arc::new(TlsManager::init(&cfg.ca_dir, &cfg.ca_key_secret)?);

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

// ─── 头解析 ────────────────────────────────────────────────────────────────

/// 读取下一个请求头的结果
enum HeadOutcome {
    /// 请求头已收全（body 字节在 remaining 中，由事务处理阶段按语义消费/归还）
    Head(ReqHeader, BytesMut),
    /// 对端在完整请求头到达前正常关闭了连接（EOF，含 TLS 无 close_notify 的场景）
    Closed,
    /// 直到 deadline 都没有等到完整请求头
    TimedOut,
}

/// 从 stream 中持续读取数据，直到 parser 弹出一个完整请求头、
/// 对端正常关闭连接，或者到达 deadline。
async fn read_next_header<S>(
    stream: &mut S,
    parser: &mut HeaderPaser,
    deadline: Instant,
) -> Result<HeadOutcome>
where
    S: AsyncRead + Unpin,
{
    if let Some((head, remaining)) = parser.try_pop()? {
        return Ok(HeadOutcome::Head(head, remaining));
    }

    loop {
        let mut buf = [0u8; READ_BUF];
        let n = match timeout_at(deadline, stream.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(HeadOutcome::Closed);
            }
            Ok(Err(e)) => return Err(e).context("read failed"),
            Err(_) => return Ok(HeadOutcome::TimedOut),
        };
        if n == 0 {
            return Ok(HeadOutcome::Closed);
        }
        parser.push(&buf[..n])?;
        if let Some((head, remaining)) = parser.try_pop()? {
            return Ok(HeadOutcome::Head(head, remaining));
        }
    }
}

// ─── 连接处理 ───────────────────────────────────────────────────────────────

/// 连接入口：读首个请求头，按是否 CONNECT 决定本地 TLS(MITM) 握手，
/// 之后统一进入 keep-alive 转发循环（泛型单态化：TcpStream / TlsStream<TcpStream>）。
async fn handle_connection(mut socket: TcpStream, shared: Arc<Shared>) -> Result<()> {
    let mut parser = HeaderPaser::new();
    let deadline = Instant::now() + FIRST_REQUEST_TIMEOUT;
    let (header, remaining) = match read_next_header(&mut socket, &mut parser, deadline).await? {
        HeadOutcome::Head(h, r) => (h, r),
        HeadOutcome::Closed => return Ok(()),
        HeadOutcome::TimedOut => bail!("timed out waiting for first request"),
    };

    if header.is_connect() {
        let authority = header.path.trim();
        anyhow::ensure!(!authority.is_empty(), "CONNECT without authority");
        let (host, _port) = split_host_port(authority).context("invalid CONNECT authority")?;
        anyhow::ensure!(!host.is_empty(), "CONNECT without authority");

        socket
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        socket.flush().await?;

        let mut tls_stream = timeout(TLS_ACCEPT_TIMEOUT, shared.tls.accept(socket, Some(host)))
            .await
            .context("timed out establishing TLS with client")?
            .context("TLS handshake failed")?;

        // 隧道内重新解析真实请求头（与 TCP 阶段的 over-read 无关）
        let mut tls_parser = HeaderPaser::new();
        let deadline = Instant::now() + TLS_FIRST_REQUEST_TIMEOUT;
        let (h2, r2) = match read_next_header(&mut tls_stream, &mut tls_parser, deadline).await? {
            HeadOutcome::Head(h, r) => (h, r),
            HeadOutcome::Closed => return Ok(()),
            HeadOutcome::TimedOut => bail!("timed out waiting for tunneled request"),
        };

        serve(tls_stream, tls_parser, h2, r2, true, shared).await
    } else {
        serve(socket, parser, header, remaining, false, shared).await
    }
}

/// 统一的 keep-alive 循环：明文 HTTP 用 TcpStream 单态化一份，
/// 隧道内 HTTPS 用 TlsStream 单态化另一份。CONNECT 不可能在此出现。
async fn serve<S>(
    mut stream: S,
    mut parser: HeaderPaser,
    mut header: ReqHeader,
    mut remaining: BytesMut,
    is_https: bool,
    shared: Arc<Shared>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        if header.is_connect() {
            bail!("unexpected CONNECT within an established stream");
        }

        let keep_alive = handle_one_request(&mut stream, &mut parser, &header, remaining, is_https, &shared).await?;

        if !keep_alive {
            break;
        }

        let deadline = Instant::now() + KEEP_ALIVE_IDLE_TIMEOUT;
        match read_next_header(&mut stream, &mut parser, deadline).await? {
            HeadOutcome::Head(h, r) => {
                header = h;
                remaining = r;
            }
            HeadOutcome::Closed | HeadOutcome::TimedOut => break,
        }
    }

    let _ = stream.shutdown().await;
    Ok(())
}

/// 单次原始读取的结果
enum RawRead {
    Data(Bytes),
    Eof,
    TimedOut,
}

async fn read_raw<S: AsyncRead + Unpin>(stream: &mut S, idle: Duration) -> Result<RawRead> {
    let mut buf = [0u8; READ_BUF];
    match timeout(idle, stream.read(&mut buf)).await {
        Ok(Ok(0)) => Ok(RawRead::Eof),
        Ok(Ok(n)) => Ok(RawRead::Data(Bytes::copy_from_slice(&buf[..n]))),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(RawRead::Eof),
        Ok(Err(e)) => Err(e).context("read failed"),
        Err(_) => Ok(RawRead::TimedOut),
    }
}

/// chunk 分块行 / trailer 行长度上限（防御畸形输入）
const CHUNK_LINE_MAX: usize = 64 * 1024;

/// 请求体范围判定：驱动"何时完成请求体（EOS）"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyExtent {
    /// 无请求体（含 Content-Length: 0）
    NoBody,
    /// Content-Length 计数的定长 body
    ContentLength(u64),
    /// Transfer-Encoding: chunked（原始字节透传，扫描分帧结束点）
    Chunked,
}

/// 从已解析请求头判定请求体范围（遵循 RFC 9112）：
/// Transfer-Encoding 覆盖 Content-Length；仅 chunked 定义分帧。
fn body_extent(headers: &[(String, String)]) -> Result<BodyExtent> {
    let mut cl: Option<u64> = None;
    let mut chunked = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length") {
            let v = v.trim();
            let n: u64 = v
                .parse()
                .with_context(|| format!("invalid content-length: {v:?}"))?;
            if cl.is_some() {
                bail!("duplicate content-length headers");
            }
            cl = Some(n);
        } else if k.eq_ignore_ascii_case("transfer-encoding")
            && v.split(',').any(|t| t.trim().eq_ignore_ascii_case("chunked"))
        {
            chunked = true;
        }
    }
    if chunked {
        return Ok(BodyExtent::Chunked);
    }
    Ok(match cl {
        Some(0) | None => BodyExtent::NoBody,
        Some(n) => BodyExtent::ContentLength(n),
    })
}

/// 拆分头部超读字节：返回 (属于本请求 body 的前缀长度, 超出部分长度)。
/// 无 body 请求的超读字节全部是下一请求（keep-alive 流水线），须归还 parser。
fn split_body_prefix(remaining: &[u8], extent: &BodyExtent) -> (usize, usize) {
    match extent {
        BodyExtent::NoBody => (0, remaining.len()),
        BodyExtent::ContentLength(n) => {
            let take = (remaining.len() as u64).min(*n) as usize;
            (take, remaining.len() - take)
        }
        BodyExtent::Chunked => (remaining.len(), 0),
    }
}

/// 泵送阶段 body 进度跟踪：Content-Length 计数 或 chunked 结束扫描。
enum PumpTracker {
    ContentLength(u64),
    Chunked(ChunkedEndScanner),
}

impl PumpTracker {
    fn new(extent: &BodyExtent) -> Option<Self> {
        match extent {
            BodyExtent::NoBody => None,
            BodyExtent::ContentLength(n) => Some(Self::ContentLength(*n)),
            BodyExtent::Chunked => Some(Self::Chunked(ChunkedEndScanner::new())),
        }
    }

    /// 处理一段字节：返回 Some(take) 表示请求体已结束，本段前 take 字节属于
    /// body（其后为下一请求字节）；None 表示尚未结束，本段全部属于 body。
    fn push(&mut self, data: &[u8]) -> Result<Option<usize>> {
        match self {
            Self::ContentLength(left) => {
                let take = (*left as usize).min(data.len());
                *left -= take as u64;
                Ok(if *left == 0 { Some(take) } else { None })
            }
            Self::Chunked(scanner) => scanner.feed(data),
        }
    }
}

/// chunked 结束扫描状态
enum ChunkState {
    /// 等待一行 chunk-size（可带扩展）
    ExpectSizeLine,
    /// 跳过 chunk 数据（按声明长度计数）
    SkipData,
    /// 等待 chunk 数据后的 CRLF
    ExpectChunkCrlf,
    /// 0 长度块之后：逐行扫 trailer 区，首个空行即结束
    TailAfterZero,
}

/// chunked 原始字节流的结束检测状态机。
/// 只追踪分帧边界，不解码 chunk 内容：数据按字节精确跳过，
/// 因此 payload 内出现 "0\r\n\r\n" 之类的序列不会被误判。
/// 结束点包含末尾的 "0\r\n\r\n"（兼容 trailer 区），之后字节属下一请求。
struct ChunkedEndScanner {
    state: ChunkState,
    buf: BytesMut,
    /// 已确认属于 body 且已消费的字节数（整个流内）
    consumed: u64,
    skip_remaining: u64,
    done: bool,
}

impl ChunkedEndScanner {
    fn new() -> Self {
        Self {
            state: ChunkState::ExpectSizeLine,
            buf: BytesMut::with_capacity(256),
            consumed: 0,
            skip_remaining: 0,
            done: false,
        }
    }

    /// 喂入一段原始字节（调用方应把同一段字节转发给 worker）。
    /// 返回 Some(take)：请求体已结束，本段前 take 字节属于 body，
    /// 本段 take.. 之后为下一请求字节（调用方归还 parser）；
    /// None：请求体未结束，本段全部属于 body。
    fn feed(&mut self, chunk: &[u8]) -> Result<Option<usize>> {
        if self.done {
            return Ok(Some(0));
        }
        let base = self.consumed;
        self.buf.extend_from_slice(chunk);

        loop {
            match self.state {
                ChunkState::ExpectSizeLine => match find_bytes(&self.buf, b"\r\n") {
                    Some(p) => {
                        let size = parse_chunk_size(&self.buf[..p])?;
                        let line_len = p + 2;
                        self.buf.advance(line_len);
                        self.consumed += line_len as u64;
                        self.state = if size == 0 {
                            ChunkState::TailAfterZero
                        } else {
                            self.skip_remaining = size;
                            ChunkState::SkipData
                        };
                    }
                    None => {
                        if self.buf.len() > CHUNK_LINE_MAX {
                            bail!("malformed chunked body: chunk size line too long");
                        }
                        return Ok(None);
                    }
                },
                ChunkState::SkipData => {
                    let skip = self.skip_remaining.min(self.buf.len() as u64) as usize;
                    if skip > 0 {
                        self.buf.advance(skip);
                        self.consumed += skip as u64;
                        self.skip_remaining -= skip as u64;
                    }
                    if self.skip_remaining == 0 {
                        self.state = ChunkState::ExpectChunkCrlf;
                    } else {
                        return Ok(None);
                    }
                }
                ChunkState::ExpectChunkCrlf => {
                    if self.buf.len() >= 2 {
                        if &self.buf[..2] == b"\r\n" {
                            self.buf.advance(2);
                            self.consumed += 2;
                            self.state = ChunkState::ExpectSizeLine;
                        } else {
                            bail!("malformed chunked body: missing CRLF after chunk data");
                        }
                    } else {
                        return Ok(None);
                    }
                }
                ChunkState::TailAfterZero => {
                    // 0 长度块之后：trailer 区逐行扫描，首个空行（直接 CRLF）即结束
                    if self.buf.len() >= 2 && &self.buf[..2] == b"\r\n" {
                        let end = self.consumed + 2;
                        self.done = true;
                        let rel = (end.saturating_sub(base) as usize).min(chunk.len());
                        return Ok(Some(rel));
                    }
                    match find_bytes(&self.buf, b"\r\n") {
                        Some(p) => {
                            let line_len = p + 2;
                            if line_len > CHUNK_LINE_MAX {
                                bail!("malformed chunked body: trailer too long");
                            }
                            self.buf.advance(line_len);
                            self.consumed += line_len as u64;
                        }
                        None => {
                            if self.buf.len() > CHUNK_LINE_MAX {
                                bail!("malformed chunked body: trailer too long");
                            }
                            return Ok(None);
                        }
                    }
                }
            }
        }
    }
}

/// 子串查找（无内存分配）
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 解析 chunk-size 行（允许 chunk 扩展 "1a;ext=x\r\n"）
fn parse_chunk_size(line: &[u8]) -> Result<u64> {
    let hex_len = line
        .iter()
        .position(|&b| !b.is_ascii_hexdigit())
        .unwrap_or(line.len());
    if hex_len == 0 {
        bail!("malformed chunked body: empty chunk size line");
    }
    let hex_str = std::str::from_utf8(&line[..hex_len])
        .map_err(|_| anyhow!("malformed chunked body: invalid chunk size"))?;
    u64::from_str_radix(hex_str, 16).map_err(|_| anyhow!("malformed chunked body: invalid chunk size"))
}

/// 转发一段字节给 worker（带 96s 上限，防上传卡死）
async fn send_to_worker(tx: &tokio::sync::mpsc::Sender<Bytes>, data: &[u8]) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    if timeout(BODY_IDLE_TIMEOUT, tx.send(Bytes::copy_from_slice(data)))
        .await
        .map_err(|_| anyhow!("worker upload stalled"))?
        .is_err()
    {
        bail!("worker body channel closed unexpectedly");
    }
    Ok(())
}

/// 处理一段属于请求体的字节：转发给 worker 并推进进度。
/// 返回 Some(take)：请求体已结束，本段前 take 字节属于 body（其后归 parser）；
/// None：尚未结束，本段全部属于 body。
async fn pump_chunk(
    tx: &tokio::sync::mpsc::Sender<Bytes>,
    tracker: &mut PumpTracker,
    data: &[u8],
) -> Result<Option<usize>> {
    match tracker.push(data)? {
        Some(take) => {
            send_to_worker(tx, &data[..take]).await?;
            Ok(Some(take))
        }
        None => {
            send_to_worker(tx, data).await?;
            Ok(None)
        }
    }
}

/// 转发单个请求：按 HTTP 语义判定请求体范围，读完浏览器请求体后立即以
/// EOS 完成 worker 请求体，再等待响应并回传。
/// Cloudflare edge 在请求体未完成前不会交付响应，因此不能"等响应再 EOS"。
///   - 上行：头帧（raw + https 标志）→ body 帧（头部超读字节 + 浏览器流）→ EOS；
///   - 请求体边界：Content-Length 按计数、chunked 按四态扫描（原始字节透传），
///     无 body 请求立即 EOS；超读字节中不属于 body 的部分归还 parser
///     （keep-alive 流水线）；浏览器中途 EOF 照发 EOS（截断，worker 侧报错）；
///   - 下行：响应帧流解包写回浏览器。
///
/// 返回是否可复用连接（keep-alive）：relay 成功且客户端未断开即可复用。
async fn handle_one_request<S>(
    stream: &mut S,
    parser: &mut HeaderPaser,
    header: &ReqHeader,
    mut remaining: BytesMut,
    is_https: bool,
    shared: &Arc<Shared>,
) -> Result<bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let algo = *shared
        .algo
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // ---------- 请求体范围判定 ----------
    let extent = body_extent(&header.headers)?;

    // ---------- 首帧：raw 头部 + https 标志（零重建） ----------
    let mut head_frame = BytesMut::with_capacity(header.raw.len() + 1);
    head_frame.extend_from_slice(&header.raw);
    head_frame.extend_from_slice(&[if is_https { 1 } else { 0 }]);
    let head_frame = head_frame.freeze();

    // ---------- 超读字节拆分：body 前缀经通道转发，其余归还 parser ----------
    let (prefix_len, _push_len) = split_body_prefix(&remaining, &extent);
    let body_prefix = remaining.split_to(prefix_len).freeze();
    if !remaining.is_empty() {
        parser.push(&remaining)?;
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Bytes>(16);
    let key16 = shared.key16;
    let key32 = shared.key32;
    let body_stream = async_stream::stream! {
        match enc_frame(&head_frame, algo, &key16, &key32) {
            Ok(frame) => yield Ok(frame),
            Err(e) => { yield Err(e); return; }
        }
        while let Some(bytes) = rx.recv().await {
            if bytes.is_empty() {
                continue;
            }
            match enc_frame(&bytes, algo, &key16, &key32) {
                Ok(frame) => yield Ok(frame),
                Err(e) => { yield Err(e); return; }
            }
        }
        // 零长帧 = EOS，请求体结束
        yield Ok(Bytes::from(make_frame(b"")));
    };

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

    let resp_fut = pref_client
        .as_ref()
        .unwrap_or(&shared.client)
        .post(url)
        .bearer_auth(gen_auth_token(&shared.token_base))
        .header("Content-Type", "application/octet-stream")
        .body(reqwest::Body::wrap_stream(body_stream))
        .send();
    tokio::pin!(resp_fut);

    // ---------- 泵送阶段：读完浏览器请求体即完成（EOS），再等响应 ----------
    // 响应在请求体未完成前不会到达（edge 契约），因此不再"等响应才发 EOS"。
    // select 中仍轮询 resp_fut：驱动 body_stream（消费通道，防背压）的同时
    // 及时上报传输层错误；本地 dev（无 edge）下响应可能提前到达，暂存即可。
    let mut tracker = PumpTracker::new(&extent);
    let mut early_resp: Option<reqwest::Response> = None;
    let mut client_eof = false;
    let mut body_done = false;

    if !body_prefix.is_empty() {
        if let Some(take) = pump_chunk(&tx, tracker.as_mut().unwrap(), &body_prefix).await? {
            if take < body_prefix.len() {
                parser.push(&body_prefix[take..])?;
            }
            body_done = true;
        }
    }

    if tracker.is_some() && !body_done {
        loop {
            tokio::select! {
                biased;
                r = read_raw(stream, BODY_IDLE_TIMEOUT) => {
                    match r? {
                        RawRead::Data(data) => {
                            match pump_chunk(&tx, tracker.as_mut().unwrap(), &data).await? {
                                Some(take) => {
                                    if take < data.len() {
                                        parser.push(&data[take..])?;
                                    }
                                    break;
                                }
                                None => {}
                            }
                        }
                        RawRead::Eof => {
                            client_eof = true;
                            break;
                        }
                        RawRead::TimedOut => bail!("browser body idle timeout"),
                    }
                }
                res = &mut resp_fut, if early_resp.is_none() => {
                    match res {
                        Ok(r) => early_resp = Some(r),
                        Err(e) => return Err(anyhow::Error::new(e).context("worker request failed")),
                    }
                }
            }
        }
    }
    drop(tx); // 通知 worker 侧请求体结束（EOS 帧）

    // ---------- 响应阶段：等待 worker 响应并回传 ----------
    let resp = match early_resp {
        Some(r) => r,
        None => match timeout(FRAME_IDLE_TIMEOUT, &mut resp_fut).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return Err(anyhow::Error::new(e).context("worker request failed")),
            Err(_) => {
                eprintln!("proxy: worker response idle timeout");
                write_502(stream).await?;
                return Ok(false);
            }
        },
    };

    if !resp.status().is_success() {
        eprintln!(
            "worker request error: {}, body: {:.1024}...",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        write_502(stream).await?;
        return Ok(false);
    }

    if !relay_response(stream, resp, algo, shared, parser).await? {
        return Ok(false);
    }

    Ok(!client_eof)
}

/// 压缩+加密并打包成一帧（body_stream 专用小工具）。
fn enc_frame(data: &[u8], algo: ProxyAlgo, key16: &[u8], key32: &[u8]) -> std::io::Result<Bytes> {
    let enc = encode_chunk(data, algo.compressor, algo.aead, key16, key32)
        .map_err(|e| std::io::Error::other(format!("encode failed: {e}")))?;
    Ok(Bytes::from(make_frame(&enc)))
}

// ─── 响应回传 ───────────────────────────────────────────────────────────────

/// 把 worker 的帧流解包写回浏览器。帧协议（FrameCache/decode_chunk）是
/// 私有最小化二进制协议，无需语义解析；同时探测客户端提前断开，
/// 多读到的字节交还 parser 供 keep-alive 下一轮使用。
async fn relay_response<S>(
    stream: &mut S,
    resp: reqwest::Response,
    algo: ProxyAlgo,
    shared: &Arc<Shared>,
    parser: &mut HeaderPaser,
) -> Result<bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut frame_cache = FrameCache::new();
    let mut resp_stream = Box::pin(resp.bytes_stream());
    let mut wrote = false;

    loop {
        tokio::select! {
            chunk = timeout(FRAME_IDLE_TIMEOUT, resp_stream.next()) => {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(_) => {
                        eprintln!("proxy: frame idle timeout (no data for {:?})", FRAME_IDLE_TIMEOUT);
                        if !wrote {
                            write_502(stream).await?;
                        }
                        return Ok(false);
                    }
                };

                let bytes = match chunk {
                    Some(Ok(b)) => b,
                    Some(Err(e)) => {
                        eprintln!("proxy: frame read error: {e}");
                        return Ok(false);
                    }
                    None => {
                        eprintln!("proxy: stream ended without EOS (truncated)");
                        if !wrote {
                            write_502(stream).await?;
                        }
                        return Ok(false);
                    }
                };

                frame_cache.push(&bytes);
                loop {
                    match frame_cache.try_pop() {
                        Ok(Frame::Frame(raw_enc)) => {
                            let raw = decode_chunk(
                                &raw_enc,
                                algo.compressor,
                                algo.aead,
                                &shared.key16,
                                &shared.key32,
                            )
                            .context("decrypt/decompress failed")?;

                            stream.write_all(&raw).await?;
                            wrote = true;
                        }
                        Ok(Frame::None) => break,
                        Ok(Frame::Eos) => {
                            // 零长帧 = 流正常完成
                            stream.flush().await?;
                            return Ok(true);
                        }
                        Err(e) => {
                            eprintln!("proxy: frame protocol error: {e}");
                            if !wrote {
                                write_502(stream).await?;
                            }
                            return Ok(false);
                        }
                    }
                }
            }
            closed = client_closed(stream, parser) => {
                if closed? {
                    return Ok(false);
                }
            }
        }
    }
}

/// 探测客户端是否已断开（读取可用字节）；未断开时字节存入 parser。
async fn client_closed<S: AsyncRead + Unpin>(
    stream: &mut S,
    parser: &mut HeaderPaser,
) -> Result<bool> {
    let mut buf = [0u8; READ_BUF];
    match stream.read(&mut buf).await {
        Ok(0) | Err(_) => Ok(true),
        Ok(n) => {
            parser.push(&buf[..n])?;
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

// ─── 错误分类（日志降噪） ─────────────────────────────────────────────────────

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

#[cfg(test)]
mod tests {
    use super::*;

    /// 头帧组装：raw 头部 + https 标志位（末尾字节），与 worker 契约一致
    #[test]
    fn test_head_frame_layout() {
        let raw = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let mut frame = BytesMut::from(&raw[..]);
        frame.extend_from_slice(&[1u8]); // https
        assert_eq!(&frame[..frame.len() - 1], raw);
        assert_eq!(frame[frame.len() - 1], 1);

        let mut frame2 = BytesMut::from(&raw[..]);
        frame2.extend_from_slice(&[0u8]); // http
        assert_eq!(frame2[frame2.len() - 1], 0);
    }

    /// enc_frame：encode_chunk + make_frame 管线，空负载仍加密为非零长帧
    #[test]
    fn test_enc_frame_pipeline() {
        let algo = ProxyAlgo::new(ProxyCompressor::default(), ProxyAead::default());
        let key16 = [0x42u8; 16];
        let key32 = [0x7Eu8; 32];

        // 非空负载：帧 = [4B 长度 | 加密负载]，长度与内容一致
        let frame = enc_frame(b"hello", algo, &key16, &key32).unwrap();
        assert!(frame.len() > 4);
        assert_eq!(
            frame.len(),
            4 + u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize
        );

        // 空负载：加密后仍是非零长帧（EOS 标记是裸 make_frame(b"")，不经过 enc_frame）
        let eos = enc_frame(b"", algo, &key16, &key32).unwrap();
        let payload_len = u32::from_be_bytes(eos[..4].try_into().unwrap()) as usize;
        assert!(payload_len > 0);
        let dec = decode_chunk(&eos[4..], algo.compressor, algo.aead, &key16, &key32).unwrap();
        assert!(dec.is_empty());
    }

    fn test_cfg(domain: &str) -> ProxyConfig {
        ProxyConfig {
            port: 0,
            domain: domain.to_string(),
            use_https: false,
            auth_key: "test-key".to_string(),
            ca_dir: std::env::temp_dir().join(format!("fp-proxy-test-{}", std::process::id())),
            ca_key_secret: [0u8; 32],
            compressor: ProxyCompressor::default(),
            aead: ProxyAead::default(),
            pref_ip: None,
        }
    }

    /// domain 携带端口 → 拒绝（token 派生与 worker env 不一致会全链路 401）
    #[test]
    fn test_proxy_new_rejects_domain_with_port() {
        assert!(Proxy::new(test_cfg("example.com:8080")).is_err());
        assert!(Proxy::new(test_cfg("[::1]:8443")).is_err());
    }

    /// 纯 host domain → 正常构造
    #[test]
    fn test_proxy_new_accepts_pure_domain() {
        let _ = Proxy::new(test_cfg("free-proxy.example.com")).expect("pure domain must pass");
    }

    // ─── body_extent / split_body_prefix ────────────────────────────────────────

    fn hdr(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_body_extent_no_body() {
        assert_eq!(
            body_extent(&hdr(&[("Host", "e.com")])).unwrap(),
            BodyExtent::NoBody
        );
        assert_eq!(
            body_extent(&hdr(&[("Content-Length", "0")])).unwrap(),
            BodyExtent::NoBody
        );
    }

    #[test]
    fn test_body_extent_content_length() {
        assert_eq!(
            body_extent(&hdr(&[("Content-Length", "11")])).unwrap(),
            BodyExtent::ContentLength(11)
        );
        assert_eq!(
            body_extent(&hdr(&[("content-length", "  42 ")])).unwrap(),
            BodyExtent::ContentLength(42)
        );
    }

    #[test]
    fn test_body_extent_invalid_or_duplicate_cl() {
        assert!(body_extent(&hdr(&[("Content-Length", "abc")])).is_err());
        assert!(body_extent(&hdr(&[("Content-Length", "1"), ("Content-Length", "2")])).is_err());
    }

    #[test]
    fn test_body_extent_chunked_overrides_cl() {
        assert_eq!(
            body_extent(&hdr(&[("Transfer-Encoding", "chunked")])).unwrap(),
            BodyExtent::Chunked
        );
        assert_eq!(
            body_extent(&hdr(&[
                ("Transfer-Encoding", "chunked"),
                ("Content-Length", "5")
            ]))
            .unwrap(),
            BodyExtent::Chunked
        );
        assert_eq!(
            body_extent(&hdr(&[("Transfer-Encoding", "gzip, chunked")])).unwrap(),
            BodyExtent::Chunked
        );
        assert_eq!(
            body_extent(&hdr(&[("Transfer-Encoding", "identity")])).unwrap(),
            BodyExtent::NoBody
        );
    }

    #[test]
    fn test_split_body_prefix_cases() {
        assert_eq!(split_body_prefix(b"GET /n", &BodyExtent::NoBody), (0, 6));
        assert_eq!(
            split_body_prefix(b"hello", &BodyExtent::ContentLength(10)),
            (5, 0)
        );
        assert_eq!(
            split_body_prefix(b"hello", &BodyExtent::ContentLength(5)),
            (5, 0)
        );
        assert_eq!(
            split_body_prefix(b"hello world", &BodyExtent::ContentLength(5)),
            (5, 6)
        );
        assert_eq!(
            split_body_prefix(b"hello world", &BodyExtent::Chunked),
            (11, 0)
        );
    }

    // ─── ChunkedEndScanner ──────────────────────────────────────────────────────

    /// 按 parts 分段喂入，返回 (累计转发的 body 字节数, 是否检测到结束)
    fn scan_parts(parts: &[&[u8]]) -> (usize, bool) {
        let mut scanner = ChunkedEndScanner::new();
        let mut forwarded = 0usize;
        for p in parts {
            match scanner.feed(p).unwrap() {
                Some(t) => {
                    forwarded += t;
                    return (forwarded, true);
                }
                None => forwarded += p.len(),
            }
        }
        (forwarded, false)
    }

    #[test]
    fn test_chunked_single_chunk() {
        let wire: &[u8] = b"4\r\nabcd\r\n0\r\n\r\n";
        assert_eq!(scan_parts(&[wire]), (14, true));
    }

    #[test]
    fn test_chunked_multiple_chunks() {
        let wire: &[u8] = b"5\r\nhello\r\n6\r\nworld!\r\n0\r\n\r\n";
        assert_eq!(scan_parts(&[wire]), (26, true));
    }

    #[test]
    fn test_chunked_empty_body() {
        let wire: &[u8] = b"0\r\n\r\n";
        assert_eq!(scan_parts(&[wire]), (5, true));
    }

    #[test]
    fn test_chunked_chunk_extension() {
        let wire: &[u8] = b"1a;ext=1\r\n01234567890123456789012345\r\n0\r\n\r\n";
        assert_eq!(scan_parts(&[wire]), (43, true));
    }

    #[test]
    fn test_chunked_trailers() {
        let wire: &[u8] = b"4\r\nabcd\r\n0\r\nX-T: v\r\n\r\n";
        assert_eq!(scan_parts(&[wire]), (22, true));
    }

    #[test]
    fn test_chunked_body_data_looks_like_terminator() {
        // chunk 数据里含 "0\r\n\r\n" 不得误判结束
        let wire: &[u8] = b"6\r\n0\r\n\r\nX\r\n0\r\n\r\n";
        assert_eq!(scan_parts(&[wire]), (16, true));
    }

    #[test]
    fn test_chunked_split_across_feeds() {
        let parts: [&[u8]; 6] = [b"4\r\n", b"ab", b"cd\r\n", b"0\r\n", b"\r", b"\n"];
        assert_eq!(scan_parts(&parts), (14, true));
    }

    #[test]
    fn test_chunked_feed_after_done_returns_zero() {
        let mut scanner = ChunkedEndScanner::new();
        assert_eq!(scanner.feed(b"4\r\nabcd\r\n0\r\n\r\n").unwrap(), Some(14));
        assert_eq!(scanner.feed(b"NEXT").unwrap(), Some(0));
    }

    #[test]
    fn test_chunked_remaining_after_end_detected() {
        // 结束点之后的字节（下一请求）不计入 body
        let wire: &[u8] = b"4\r\nabcd\r\n0\r\n\r\nGET / HTTP/1.1\r\n\r\n";
        let mut scanner = ChunkedEndScanner::new();
        assert_eq!(scanner.feed(wire).unwrap(), Some(14));
    }

    #[test]
    fn test_chunked_malformed() {
        let mut s1 = ChunkedEndScanner::new();
        assert!(s1.feed(b"zz\r\n").is_err());

        // chunk 数据后缺 CRLF
        let mut s2 = ChunkedEndScanner::new();
        assert!(s2.feed(b"4\r\nabcdX0\r\n\r\n").is_err());
    }
}

