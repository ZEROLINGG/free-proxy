// server-rs：订阅配置生成（Clash / SingBox / Base64 通用订阅）。
use axum::extract::{Path, Query};
use axum::http::{header, HeaderMap, HeaderName};
use axum::response::IntoResponse;
use serde::Deserialize;

use lib::base::{Base64, Encoder};

#[derive(Deserialize, Debug)]
pub(crate) struct SubscribeQuery {
    pub(crate) target: Option<String>,
    pub(crate) flag: Option<String>,
}

enum SubType {
    Clash,
    Base64, // v2rayN, Shadowrocket, Quantumult X, PassWall 等
    SingBox,
}

pub(crate) async fn subscribe(
    Path(port): Path<String>,
    Query(query): Query<SubscribeQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let node_name = "[Cloudflare Worker]free-proxy";
    let host = "127.0.0.1";

    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let target_param = query
        .target
        .as_deref()
        .or(query.flag.as_deref())
        .unwrap_or("")
        .to_lowercase();

    let sub_type = if target_param.contains("clash")
        || ua.contains("clash")
        || ua.contains("mihomo")
        || ua.contains("verge")
        || ua.contains("stash")
    {
        SubType::Clash
    } else if target_param.contains("singbox")
        || target_param.contains("sing-box")
        || ua.contains("sing-box")
    {
        SubType::SingBox
    } else {
        SubType::Base64
    };

    let extra_headers = [
        (
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"subscription\"",
        ),
        (
            HeaderName::from_static("profile-update-interval"),
            "2400000",
        ),
        (
            HeaderName::from_static("subscription-userinfo"),
            "upload=0; download=0; total=10737418240000000000; expire=0",
        ),
    ];

    match sub_type {
        SubType::Clash => {
            let yaml_content = format!(
                r#"proxies:
  - name: "{node_name}"
    type: http
    server: {host}
    port: {port}

proxy-groups:
  - name: "Proxy"
    type: select
    proxies:
      - "{node_name}"
      - DIRECT

rules:
  - MATCH,Proxy
"#
            );
            (
                [(header::CONTENT_TYPE, "text/yaml; charset=utf-8")],
                extra_headers,
                yaml_content,
            )
                .into_response()
        }

        SubType::SingBox => {
            let json_content = format!(
                r#"{{
  "outbounds": [
    {{
      "type": "selector",
      "tag": "Proxy",
      "outbounds": ["{node_name}", "direct"]
    }},
    {{
      "type": "http",
      "tag": "{node_name}",
      "server": "{host}",
      "server_port": {port}
    }},
    {{
      "type": "direct",
      "tag": "direct"
    }}
  ]
}}"#,
                node_name = node_name,
                host = host,
                port = port
            );
            (
                [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
                extra_headers,
                json_content,
            )
                .into_response()
        }

        SubType::Base64 => {
            let raw_links = format!("http://{}:{}#{}\n", host, port, node_name);
            let encoded_content = Base64::encode(raw_links.as_bytes()).unwrap();
            (
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                extra_headers,
                encoded_content,
            )
                .into_response()
        }
    }
}
