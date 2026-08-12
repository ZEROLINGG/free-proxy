//lib/src/http.rs
use anyhow::{Context, Result, anyhow, bail};
use bytes::BytesMut;
use httparse;
use std::borrow::Cow;

#[derive(Debug)]
pub struct HttpReq<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub version: u8,
    pub headers: Vec<(&'a str, Cow<'a, str>)>,
    pub body: &'a [u8],
}

/// 一个请求头部允许的最大字节数
const MAX_HEADER_SIZE: usize = 1024 * 1024;
/// 单个请求 body 允许的最大字节数
const MAX_BODY_SIZE: usize = 20 * 1024 * 1024;

/// 单个 HTTP 请求允许的最大 header 条目数。
/// 设为 pub(crate) 并在整个 crate 内复用，避免各处各写一份、
/// 数值不同步导致真实（header 数量较多的）请求解析失败。
pub(crate) const MAX_HEADERS: usize = 128;

/// 缓冲区允许堆积的最大字节数（一个完整请求最多 header + body）
const MAX_BUFFER_SIZE: usize = MAX_HEADER_SIZE + MAX_BODY_SIZE;

/// chunk-size 行（含扩展参数，如 ";name=value"）允许的最大长度。
/// 用于限定 chunked 解析时单次查找的窗口，避免恶意构造的海量小
/// chunk 触发 O(n^2) 的全量扫描（拒绝服务风险）。
const MAX_CHUNK_LINE_LEN: usize = 512;

/// Body 长度的确定方式
enum BodyLength {
    /// 明确长度（Content-Length）
    Fixed(usize),
    /// chunked 编码，需要扫描确定
    Chunked,
    /// 没有 body
    None,
}

pub struct HttpReqCache {
    buffer: BytesMut,
}

impl HttpReqCache {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::with_capacity(4096),
        }
    }

    /// 追加新读到的数据到缓冲区
    pub fn push(&mut self, data: &[u8]) -> Result<()> {
        if self.buffer.len() + data.len() > MAX_BUFFER_SIZE {
            bail!(
                "HttpReqCache buffer overflow: {} + {} > {}",
                self.buffer.len(),
                data.len(),
                MAX_BUFFER_SIZE
            );
        }
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    /// 尝试从缓冲区弹出一个完整的 HTTP 请求（原始字节）。
    /// 若缓冲区中还残留下一个请求的字节（pipelining / keep-alive 场景），
    /// 会在下次调用时继续弹出，不会丢失。
    pub fn pop(&mut self) -> Result<Option<Vec<u8>>> {
        if self.buffer.is_empty() {
            return Ok(None);
        }

        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut req = httparse::Request::new(&mut headers);

        let header_len = match req.parse(&self.buffer) {
            Ok(httparse::Status::Complete(len)) => len,
            Ok(httparse::Status::Partial) => {
                // header 还没收完整，检查是否已经超过上限
                if self.buffer.len() > MAX_HEADER_SIZE {
                    bail!("HTTP header too large (> {} bytes)", MAX_HEADER_SIZE);
                }
                return Ok(None); // 等待更多数据
            }
            Err(e) => {
                bail!("Malformed HTTP request: {}", e);
            }
        };

        if header_len > MAX_HEADER_SIZE {
            bail!("HTTP header too large (> {} bytes)", MAX_HEADER_SIZE);
        }

        // 2. 根据 header 判断 body 长度类型
        let body_len_kind = Self::determine_body_length(&req)?;

        // 3. 计算这个请求总共需要多少字节，判断是否已经收全
        let total_len = match body_len_kind {
            BodyLength::None => header_len,
            BodyLength::Fixed(n) => {
                if n > MAX_BODY_SIZE {
                    bail!("Content-Length too large: {} > {}", n, MAX_BODY_SIZE);
                }
                header_len + n
            }
            BodyLength::Chunked => {
                match Self::find_chunked_end(&self.buffer[header_len..])? {
                    Some(body_len) => {
                        if body_len > MAX_BODY_SIZE {
                            bail!("Chunked body too large: {} > {}", body_len, MAX_BODY_SIZE);
                        }
                        header_len + body_len
                    }
                    None => return Ok(None), // chunked body 还没收全
                }
            }
        };

        if self.buffer.len() < total_len {
            return Ok(None); // body 还没收全，等待更多数据
        }

        // 4. 切出这一个完整请求，剩余数据留在 buffer 里（供下次 pop 处理粘包/keep-alive）
        let request_bytes = self.buffer.split_to(total_len);
        Ok(Some(request_bytes.to_vec()))
    }

    /// 根据 headers 判断 body 的长度确定方式
    fn determine_body_length(req: &httparse::Request) -> Result<BodyLength> {
        let mut content_length: Option<usize> = None;
        let mut is_chunked = false;

        for h in req.headers.iter() {
            if h.name.eq_ignore_ascii_case("content-length") {
                let val = std::str::from_utf8(h.value)
                    .context("Invalid Content-Length encoding")?
                    .trim();

                // RFC 7230 §3.3.2: Content-Length 的值必须是 1*DIGIT。
                // Rust 的 usize::from_str 会额外接受 "+5" 这样的形式，
                // 若与后端解析器行为不一致，可能构成请求走私（smuggling）的
                // 差异面，因此这里显式做纯数字校验。
                if val.is_empty() || !val.bytes().all(|b| b.is_ascii_digit()) {
                    bail!("Invalid Content-Length value: {:?}", val);
                }

                let n: usize = val.parse().context("Invalid Content-Length value")?;
                if let Some(prev) = content_length {
                    if prev != n {
                        bail!("Conflicting Content-Length headers (possible smuggling)");
                    }
                }
                content_length = Some(n);
            } else if h.name.eq_ignore_ascii_case("transfer-encoding") {
                let val = String::from_utf8_lossy(h.value).to_ascii_lowercase();
                if val.contains("chunked") {
                    is_chunked = true;
                }
            }
        }

        if is_chunked && content_length.is_some() {
            // RFC 7230 §3.3.3: 同时出现时必须拒绝（走私攻击特征）
            bail!("Both Transfer-Encoding: chunked and Content-Length present");
        }

        if is_chunked {
            Ok(BodyLength::Chunked)
        } else if let Some(n) = content_length {
            Ok(BodyLength::Fixed(n))
        } else {
            Ok(BodyLength::None)
        }
    }

    /// 在 chunked body 原始字节中扫描，找到完整 body（含末尾 0-chunk 和 trailer）的总长度。
    /// 只定位边界，不做真正解码。
    ///
    /// 返回：
    /// - `Ok(Some(len))`：body 已收全，长度为 len
    /// - `Ok(None)`：数据还不够，等待更多字节
    /// - `Err(_)`：数据明显畸形（如 chunk-size 行超长、无法解析），直接拒绝该连接
    fn find_chunked_end(data: &[u8]) -> Result<Option<usize>> {
        fn find_subslice(data: &[u8], pat: &[u8]) -> Option<usize> {
            data.windows(pat.len()).position(|w| w == pat)
        }

        let mut pos = 0usize;

        loop {
            let search_end = (pos + MAX_CHUNK_LINE_LEN).min(data.len());
            let window = &data[pos..search_end];
            let line_end = match find_subslice(window, b"\r\n") {
                Some(off) => pos + off,
                None => {
                    if data.len() - pos > MAX_CHUNK_LINE_LEN {
                        bail!("Malformed chunked encoding: chunk-size line too long");
                    }
                    return Ok(None); // 数据不够，等待更多字节
                }
            };

            let size_line = std::str::from_utf8(&data[pos..line_end])
                .map_err(|_| anyhow!("Malformed chunked encoding: invalid chunk-size line"))?;
            // chunk size 可能带扩展参数，如 "1a;name=value"
            let size_str = size_line.split(';').next().unwrap_or("").trim();
            let size = usize::from_str_radix(size_str, 16)
                .map_err(|_| anyhow!("Malformed chunked encoding: invalid chunk size"))?;

            let chunk_data_start = line_end + 2;

            if size == 0 {
                let rest = &data[chunk_data_start..];
                if rest.starts_with(b"\r\n") {
                    return Ok(Some(chunk_data_start + 2));
                }
                return match find_subslice(rest, b"\r\n\r\n") {
                    Some(trailer_end) => Ok(Some(chunk_data_start + trailer_end + 4)),
                    None => Ok(None), // trailer 还没收全，交由外层 MAX_BODY_SIZE 兜底防滥用
                };
            }

            let chunk_data_end = chunk_data_start
                .checked_add(size)
                .ok_or_else(|| anyhow!("Malformed chunked encoding: size overflow"))?;
            let after_chunk = chunk_data_end
                .checked_add(2)
                .ok_or_else(|| anyhow!("Malformed chunked encoding: size overflow"))?; // 数据后还有 \r\n

            if data.len() < after_chunk {
                return Ok(None); // 这个 chunk 还没收全
            }

            pos = after_chunk;
        }
    }

    /// 主动清空缓冲区
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl Default for HttpReqCache {
    fn default() -> Self {
        Self::new()
    }
}

pub fn http_parse_req<'a>(raw: &'a [u8]) -> Result<HttpReq<'a>> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut req = httparse::Request::new(&mut headers);

    let status = req.parse(raw).context("Failed to parse HTTP request")?;

    let header_len = match status {
        httparse::Status::Complete(len) => len,
        httparse::Status::Partial => return Err(anyhow!("Incomplete HTTP request")),
    };

    let method = req.method.ok_or_else(|| anyhow!("Missing HTTP method"))?;
    let path = req.path.ok_or_else(|| anyhow!("Missing HTTP path"))?;
    let version = req.version.ok_or_else(|| anyhow!("Missing HTTP version"))?;

    let mut parsed_headers = Vec::with_capacity(req.headers.len());

    for h in req.headers {
        let name = h.name;
        let value_cow = String::from_utf8_lossy(h.value);
        parsed_headers.push((name, value_cow));
    }

    let body = &raw[header_len..];

    Ok(HttpReq {
        method,
        path,
        version,
        headers: parsed_headers,
        body,
    })
}

impl<'a> HttpReq<'a> {
    /// 由 Host 头 + request-target 组合出完整 URL（origin-form → 绝对 URL；
    /// absolute-form / CONNECT authority 原样透传）。
    /// 按需调用：客户端（无需 full_url）可跳过，避免对占位协议/权威串的
    /// 严格解析造成无谓失败。
    pub fn full_url(&self, protocol: &str) -> Result<String> {
        let host = self
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("host"))
            .map(|(_, val)| val.as_ref().trim());

        UrlBuilder::new()
            .scheme(protocol)
            .host(host)
            .path(self.path)
            .build()
    }
}


#[derive(Debug, Clone, PartialEq)]
pub struct Url {
    pub scheme: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub fragment: Option<String>,
    pub username: String,
    pub password: Option<String>,
}
impl std::fmt::Display for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // IPv6 host 必须加方括号，否则 url::Url 解析失败走 fallback 输出非法 URL
        let base_str = match &self.host {
            Some(host) if host.contains(':') && !host.starts_with('[') => {
                format!("{}://[{}]", self.scheme, host)
            }
            Some(host) => format!("{}://{}", self.scheme, host),
            None => format!("{}:", self.scheme),
        };

        let mut u = match url::Url::parse(&base_str) {
            Ok(u) => u,
            Err(_) => {
                write!(f, "{}", base_str)?;
                if !self.path.is_empty() {
                    write!(f, "{}", self.path)?;
                }
                return Ok(());
            }
        };

        if let Some(port) = self.port {
            let _ = u.set_port(Some(port));
        }

        if !self.username.is_empty() {
            let _ = u.set_username(&self.username);
        }

        if let Some(password) = &self.password {
            let _ = u.set_password(Some(password));
        }

        if !self.path.is_empty() {
            u.set_path(&self.path);
        }

        if !self.query.is_empty() {
            let mut q_pairs = u.query_pairs_mut();
            for (k, v) in &self.query {
                q_pairs.append_pair(k, v);
            }
        }

        if let Some(fragment) = &self.fragment {
            u.set_fragment(Some(fragment));
        }

        write!(f, "{}", u.as_str())
    }
}



/// 解析 URL 字符串并转换为自定义的 Url 结构体
pub fn url_parse(url_str: &str) -> Result<Url> {
    let parsed = url::Url::parse(url_str)
        .map_err(|e| anyhow!("Failed to parse URL: {}", e))?;

    let query: Vec<(String, String)> = parsed
        .query_pairs()
        .into_owned()
        .collect();

    Ok(Url {
        scheme: parsed.scheme().to_string(),
        // 不用 host_str()/Host Display：两者对 IPv6 都带方括号，
        // 而下游（reqwest .resolve、域名归一化）需要无括号形式
        host: parsed.host().map(|h| match h {
            url::Host::Ipv6(addr) => addr.to_string(),
            _ => h.to_string(),
        }),
        port: parsed.port_or_known_default(),
        path: parsed.path().to_string(),
        query,
        fragment: parsed.fragment().map(String::from),
        username: parsed.username().to_string(),
        password: parsed.password().map(String::from),
    })
}



#[derive(Debug, Default)]
pub struct UrlBuilder<'a> {
    base_url: Option<&'a str>,
    scheme: Option<&'a str>,
    host: Option<&'a str>,
    port: Option<u16>,
    path: Option<&'a str>,
    raw_query: Option<&'a str>,
    query_pairs: Vec<(&'a str, &'a str)>,
    fragment: Option<&'a str>,
}

impl<'a> UrlBuilder<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置基准 URL，后续参数会在此基础上合并
    pub fn base(mut self, base: impl Into<Option<&'a str>>) -> Self {
        self.base_url = base.into();
        self
    }

    pub fn scheme(mut self, scheme: impl Into<Option<&'a str>>) -> Self {
        self.scheme = scheme.into();
        self
    }

    pub fn host(mut self, host: impl Into<Option<&'a str>>) -> Self {
        self.host = host.into();
        self
    }

    pub fn port(mut self, port: impl Into<Option<u16>>) -> Self {
        self.port = port.into();
        self
    }

    pub fn path(mut self, path: impl Into<Option<&'a str>>) -> Self {
        self.path = path.into();
        self
    }

    /// 直接设置查询字符串 (不带 '?')
    pub fn query(mut self, query: impl Into<Option<&'a str>>) -> Self {
        self.raw_query = query.into();
        self
    }

    /// 追加查询参数
    pub fn append_query(mut self, key: &'a str, value: &'a str) -> Self {
        self.query_pairs.push((key, value));
        self
    }

    pub fn fragment(mut self, fragment: impl Into<Option<&'a str>>) -> Self {
        self.fragment = fragment.into();
        self
    }

    pub fn https(self, flag: bool) -> Self {
        if flag { self.scheme("https") } else { self.scheme("http") }
    }

    /// 执行构建并生成最终的 URL 字符串
    pub fn build(self) -> Result<String> {
        let mut url = match self.base_url {
            Some(base) => url::Url::parse(base)
                .map_err(|e| anyhow!("Invalid base URL: {}", e))?,
            None => {
                if let Some(p) = self.path {
                    if let Ok(u) = url::Url::parse(p) {
                        u
                    } else {
                        self.create_base_from_components()?
                    }
                } else {
                    self.create_base_from_components()?
                }
            }
        };

        if let Some(path_str) = self.path {
            if self.base_url.is_some() || url::Url::parse(path_str).is_err() {
                url = url.join(path_str)
                    .map_err(|e| anyhow!("Failed to join path: {}", e))?;
            }
        }

        if let Some(scheme) = self.scheme {
            url.set_scheme(scheme).map_err(|_| anyhow!("Invalid scheme"))?;
        }
        if let Some(host) = self.host {
            url.set_host(Some(host)).map_err(|_| anyhow!("Invalid host"))?;
        }
        if let Some(port) = self.port {
            let _ = url.set_port(Some(port));
        }

        if let Some(q) = self.raw_query {
            let existing_query = url.query().unwrap_or("").to_string();
            let new_query = if existing_query.is_empty() {
                q.to_string()
            } else {
                format!("{}&{}", existing_query, q)
            };
            url.set_query(Some(&new_query));
        }

        if !self.query_pairs.is_empty() {
            let mut q_pairs = url.query_pairs_mut();
            for (k, v) in self.query_pairs {
                q_pairs.append_pair(k, v);
            }
        }

        if let Some(fragment) = self.fragment {
            url.set_fragment(Some(fragment));
        }

        Ok(url.to_string())
    }

    fn create_base_from_components(&self) -> Result<url::Url> {
        let scheme = self.scheme.unwrap_or("http");
        let host = self.host.ok_or_else(|| anyhow!("Missing host in URL builder"))?;
        let dummy = format!("{}://{}", scheme, host);
        url::Url::parse(&dummy).map_err(|_| anyhow!("Failed to create URL from scheme and host"))
    }
}

