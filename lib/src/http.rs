// lib/src/http.rs
use anyhow::{Context, Result, anyhow, bail};
use bytes::BytesMut;
use httparse;

/// 一个请求头部允许的最大字节数
const MAX_HEADER_SIZE: usize = 1024 * 1024;

/// 单个 HTTP 请求允许的最大 header 条目数。
pub(crate) const MAX_HEADERS: usize = 128;

pub struct ReqHeader {
    pub method: String,
    pub path: String,
    pub version: u8,
    pub headers: Vec<(String, String)>,
    pub raw: BytesMut,
}

impl ReqHeader {
    pub fn is_connect(&self) -> bool {
        self.method.eq_ignore_ascii_case("CONNECT")
    }

    /// 判断是否为 WebSocket 升级请求
    /// 条件: Connection 头包含 "upgrade" 且 Upgrade 头值为 "websocket"（不区分大小写）
    pub fn is_websocket_upgrade(&self) -> bool {
        let mut has_upgrade_connection = false;
        let mut has_websocket_upgrade = false;
        for (k, v) in &self.headers {
            if k.eq_ignore_ascii_case("connection") {
                if v.split(',').any(|t| t.trim().eq_ignore_ascii_case("upgrade")) {
                    has_upgrade_connection = true;
                }
            }
            if k.eq_ignore_ascii_case("upgrade") && v.trim().eq_ignore_ascii_case("websocket") {
                has_websocket_upgrade = true;
            }
        }
        has_upgrade_connection && has_websocket_upgrade
    }

    /// 提取 Sec-WebSocket-Key 头的值
    pub fn websocket_key(&self) -> Option<&str> {
        self.headers.iter().find_map(|(k, v)| {
            if k.eq_ignore_ascii_case("sec-websocket-key") {
                Some(v.trim())
            } else {
                None
            }
        })
    }
}

pub struct HeaderPaser {
    buffer: BytesMut,
}

impl HeaderPaser {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::with_capacity(4096),
        }
    }

    pub fn push(&mut self, data: &[u8]) -> Result<()> {
        if self.buffer.len() + data.len() > 1024 * 1024 * 3 {
            bail!(
                "HeaderPase buffer overflow: {} + {} > {}",
                self.buffer.len(),
                data.len(),
                1024 * 1024 * 3
            );
        }
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    // 尝试弹出已解析完整的请求头
    pub fn try_pop(&mut self) -> Result<Option<(ReqHeader, BytesMut)>> {
        if self.buffer.is_empty() {
            return Ok(None);
        }

        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut req = httparse::Request::new(&mut headers);

        let header_len = match req.parse(&self.buffer) {
            Ok(httparse::Status::Complete(len)) => len,
            Ok(httparse::Status::Partial) => {
                if self.buffer.len() > MAX_HEADER_SIZE {
                    bail!("HTTP header too large (> {} bytes)", MAX_HEADER_SIZE);
                }
                return Ok(None);
            }
            Err(e) => {
                bail!("Malformed HTTP request: {}", e);
            }
        };

        if header_len > MAX_HEADER_SIZE {
            bail!("HTTP header too large (> {} bytes)", MAX_HEADER_SIZE);
        }

        let method = req.method.ok_or_else(|| anyhow!("Missing HTTP method"))?.to_string();
        let path = req.path.ok_or_else(|| anyhow!("Missing HTTP path"))?.to_string();
        let version = req.version.ok_or_else(|| anyhow!("Missing HTTP version"))?;

        let mut parsed_headers = Vec::with_capacity(req.headers.len());
        for h in req.headers.iter() {
            let name = h.name.to_string();
            let value = String::from_utf8_lossy(h.value).into_owned();
            parsed_headers.push((name, value));
        }

        let raw = self.buffer.split_to(header_len);

        let remaining = std::mem::take(&mut self.buffer);

        let req_header = ReqHeader {
            method,
            path,
            version,
            headers: parsed_headers,
            raw,
        };

        Ok(Some((req_header, remaining)))
    }
}


/// 服务端专用：头帧的轻量借用解析。
pub struct ParsedHead<'a> {
    pub method: &'a str,
    /// request-target：origin-form（"/x?y"）或 absolute-form（"http://..."）
    pub target: &'a str,
    /// Host 头值（HTTP/1.1 必需，缺失时由调用方决定拒绝）
    pub host: Option<&'a str>,
    /// 借用视图的 (name, value)，零拷贝
    pub headers: Vec<(&'a str, &'a str)>,
}

/// 解析一个完整的请求头（含结尾 \r\n\r\n），全部借用输入字节。
pub fn parse_head(raw: &[u8]) -> Result<ParsedHead<'_>> {
    if raw.len() > MAX_HEADER_SIZE {
        bail!("HTTP header too large ({} bytes)", raw.len());
    }

    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut req = httparse::Request::new(&mut headers);

    match req.parse(raw) {
        Ok(httparse::Status::Complete(_len)) => {}
        Ok(httparse::Status::Partial) => bail!("Incomplete HTTP request"),
        Err(e) => bail!("Malformed HTTP request: {}", e),
    }

    let method = req.method.ok_or_else(|| anyhow!("Missing HTTP method"))?;
    let target = req.path.ok_or_else(|| anyhow!("Missing HTTP path"))?;

    let mut parsed_headers = Vec::with_capacity(req.headers.len());
    let mut host = None;
    for h in req.headers.iter() {
        let name = h.name;
        let value = std::str::from_utf8(h.value).context("Invalid header encoding")?;
        if host.is_none() && name.eq_ignore_ascii_case("host") {
            host = Some(value.trim());
        }
        parsed_headers.push((name, value));
    }

    Ok(ParsedHead {
        method,
        target,
        host,
        headers: parsed_headers,
    })
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



/// 拆分 "host" / "host:port" / "[ipv6]:port" 形式的字符串。
pub fn split_host_port(s: &str) -> Result<(&str, Option<u16>)> {
    let s = s.trim();
    if s.is_empty() {
        bail!("empty host");
    }
    if let Some(rest) = s.strip_prefix('[') {
        // [ipv6]:port
        let end = rest
            .find(']')
            .ok_or_else(|| anyhow!("invalid IPv6 host: {s}"))?;
        let host = &rest[..end];
        let after = &rest[end + 1..];
        if after.is_empty() {
            return Ok((host, None));
        }
        let port_str = after
            .strip_prefix(':')
            .ok_or_else(|| anyhow!("invalid host: {s}"))?;
        let port = port_str
            .parse()
            .map_err(|_| anyhow!("invalid port in host: {s}"))?;
        return Ok((host, Some(port)));
    }
    if s.matches(':').count() >= 2 {
        // 裸 IPv6 地址（无端口）
        return Ok((s, None));
    }
    match s.rsplit_once(':') {
        Some((h, p)) => {
            if h.is_empty() {
                bail!("invalid host: {s}");
            }
            let port = p.parse().map_err(|_| anyhow!("invalid port in host: {s}"))?;
            Ok((h, Some(port)))
        }
        None => Ok((s, None)),
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
            // host 可能携带端口（"example.com:8080" / "[::1]:8443"）：
            // url::Url::set_host 会静默丢弃端口、且不接受裸 IPv6，
            // 因此拆分后分别设置，IPv6 补回方括号。
            let (host, port) = split_host_port(host).map_err(|_| anyhow!("Invalid host"))?;
            let host_for_url = if host.contains(':') && !host.starts_with('[') {
                format!("[{host}]")
            } else {
                host.to_string()
            };
            url.set_host(Some(&host_for_url))
                .map_err(|_| anyhow!("Invalid host"))?;
            if let Some(p) = port {
                let _ = url.set_port(Some(p));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_paser_roundtrip_with_remaining() {
        let mut parser = HeaderPaser::new();
        let wire = b"POST /upload HTTP/1.1\r\nHost: example.com\r\nContent-Length: 11\r\n\r\nhello worldGET /next HTTP/1.1\r\nHost: e.com\r\n\r\n";
        parser.push(wire).unwrap();

        let (head, remaining) = parser.try_pop().unwrap().unwrap();
        assert_eq!(
            &head.raw[..],
            b"POST /upload HTTP/1.1\r\nHost: example.com\r\nContent-Length: 11\r\n\r\n"
        );
        assert_eq!(&remaining[..], b"hello worldGET /next HTTP/1.1\r\nHost: e.com\r\n\r\n");
    }

    #[test]
    fn test_parse_head_origin_form() {
        let head = parse_head(b"GET /api/v1/auth?x=1 HTTP/1.1\r\nHost: example.com\r\nAccept: */*\r\n\r\n")
            .unwrap();
        assert_eq!(head.method, "GET");
        assert_eq!(head.target, "/api/v1/auth?x=1");
        assert_eq!(head.host, Some("example.com"));
        assert_eq!(head.headers.len(), 2);
        assert!(head.headers.contains(&("Accept", "*/*")));
    }

    #[test]
    fn test_parse_head_host_case_insensitive() {
        let head = parse_head(b"GET / HTTP/1.1\r\nhOsT: Example.COM\r\n\r\n").unwrap();
        assert_eq!(head.host, Some("Example.COM"));
    }

    #[test]
    fn test_parse_head_absolute_form() {
        let head =
            parse_head(b"GET http://example.com/x HTTP/1.1\r\nHost: example.com\r\n\r\n").unwrap();
        assert_eq!(head.method, "GET");
        assert_eq!(head.target, "http://example.com/x");
        assert_eq!(head.host, Some("example.com"));
    }

    #[test]
    fn test_parse_head_missing_host() {
        let head = parse_head(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(head.host, None);
    }

    #[test]
    fn test_parse_head_host_trimmed() {
        let head = parse_head(b"GET / HTTP/1.1\r\nHost:  example.com  \r\n\r\n").unwrap();
        assert_eq!(head.host, Some("example.com"));
    }

    #[test]
    fn test_parse_head_rejects_malformed() {
        assert!(parse_head(b"not an http request").is_err());
        assert!(parse_head(b"GET / HTTP/1.1\r\nHost: e.com\r\nBad Header\r\n\r\n").is_err());
    }

    #[test]
    fn test_parse_head_rejects_partial() {
        assert!(parse_head(b"GET / HTTP/1.1\r\nHost: e.com").is_err());
    }

    #[test]
    fn test_parse_head_rejects_oversize() {
        let mut wire = b"GET / HTTP/1.1\r\nHost: e.com\r\nX: ".to_vec();
        wire.extend(std::iter::repeat_n(b'a', MAX_HEADER_SIZE));
        wire.extend_from_slice(b"\r\n\r\n");
        assert!(parse_head(&wire).is_err());
    }

    #[test]
    fn test_parse_head_non_utf8_header_value() {
        let wire = b"GET / HTTP/1.1\r\nHost: e.com\r\nX-Enc: \xff\xfe\r\n\r\n";
        assert!(parse_head(wire).is_err());
    }

    /// host 字符串可携带端口（服务端 Host 头场景），拆分后正确落到 host:port
    #[test]
    fn test_url_builder_host_with_port() {
        let url = UrlBuilder::new()
            .scheme("http")
            .host("example.com:8080")
            .path("/api/v1/auth")
            .build()
            .unwrap();
        assert_eq!(url, "http://example.com:8080/api/v1/auth");

        let url2 = UrlBuilder::new()
            .https(false)
            .host("127.0.0.1:8787")
            .build()
            .unwrap();
        assert_eq!(url2, "http://127.0.0.1:8787/");

        let url3 = UrlBuilder::new()
            .host("example.com:8080")
            .path("/x")
            .build()
            .unwrap();
        assert_eq!(url3, "http://example.com:8080/x");
    }

    /// host 携带端口时，显式 port 参数优先覆盖
    #[test]
    fn test_url_builder_host_port_takes_precedence() {
        let url = UrlBuilder::new()
            .host("example.com:8080")
            .port(443)
            .path("/")
            .build()
            .unwrap();
        assert_eq!(url, "http://example.com:443/");
    }

    /// IPv6 + 端口
    #[test]
    fn test_url_builder_ipv6_host_with_port() {
        let url = UrlBuilder::new()
            .host("[::1]:8443")
            .path("/h")
            .build()
            .unwrap();
        assert_eq!(url, "http://[::1]:8443/h");
    }

    /// 非法 host（空 / 只有冒号）报错
    #[test]
    fn test_url_builder_invalid_host() {
        assert!(UrlBuilder::new().host("").build().is_err());
        assert!(UrlBuilder::new().host("example.com:").build().is_err());
        assert!(UrlBuilder::new().host(":8080").build().is_err());
    }

    #[test]
    fn test_req_header_websocket_upgrade() {
        let req = ReqHeader {
            method: "GET".to_string(),
            path: "/chat".to_string(),
            version: 1,
            headers: vec![
                ("Host".to_string(), "example.com".to_string()),
                ("Upgrade".to_string(), "websocket".to_string()),
                ("Connection".to_string(), "keep-alive, Upgrade".to_string()),
                ("Sec-WebSocket-Key".to_string(), "dGhlIHNhbXBsZSBub25jZQ==".to_string()),
            ],
            raw: BytesMut::new(),
        };

        assert!(req.is_websocket_upgrade());
        assert_eq!(req.websocket_key(), Some("dGhlIHNhbXBsZSBub25jZQ=="));

        let non_ws = ReqHeader {
            method: "GET".to_string(),
            path: "/index.html".to_string(),
            version: 1,
            headers: vec![
                ("Host".to_string(), "example.com".to_string()),
                ("Connection".to_string(), "keep-alive".to_string()),
            ],
            raw: BytesMut::new(),
        };

        assert!(!non_ws.is_websocket_upgrade());
        assert_eq!(non_ws.websocket_key(), None);
    }
}

