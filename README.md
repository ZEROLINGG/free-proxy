# free-proxy — 零成本私人专属代理

> 无需购买 VPS，无需注册域名，无需备案，仅借助 **Cloudflare 免费 Worker** 服务，即可部署一个专属于个人使用、流量几乎不受限的代理节点。

---

## 一、项目简介

`free-proxy` 是一套由「客户端 + 免费中转服务」构成的私人代理方案：

- **服务端**部署于 Cloudflare Worker 之上——该服务为 Cloudflare 提供的**免费**产品，其免费额度足以满足个人日常使用需求；
- **客户端**为运行于桌面或移动设备上的应用程序，负责数据的加密、压缩与加速处理，并通过 Worker 完成流量转发。

<div><img alt="" src="./image/screenshot-20260821-194431.png"></div>
<div><img alt="" src="./image/screenshot-20260818-164913.png"></div>

### 核心特性

- **零成本**：全部功能依托 Cloudflare 免费额度实现，无需支付月租或维护费用；
- **专属独享**：节点仅供个人使用，认证密钥仅存在于用户本人与设备之间；
- **流量优化**：内置 zstd / lz4 压缩算法，有效降低传输数据量，提升访问速度；
- **传输加密**：支持多种加密算法（ChaCha20-Poly1305、Ascon-AEAD128 等），保障传输内容的安全性；
- **智能选路**：内置「优选 IP」测速工具，自动探测与 Worker 之间延迟最低的网络路径；
- **广泛兼容**：客户端支持导出订阅链接，可直接用于 Clash、sing-box、v2rayN 等主流代理工具；
- **WebSocket 支持**：内置 WebSocket 隧道，`ws://` / `wss://` 协议下的即时通信、消息推送、协同办公等应用均可正常经由加密代理访问；
- **跨平台适配**：支持桌面端（Windows / macOS / Linux）及移动端（实验性支持）。

---

## 二、前置条件

1. 一个 **Cloudflare 账号**；
2. 一台可用于操作的**计算机**；
3. 该计算机需预先安装 **Node.js**、**Rust 工具链**及 **pnpm**。

---

## 三、快速部署指南

### 第一步：部署 Worker 服务端

```bash
# 安装构建工具（仅需执行一次）
cargo install -q "worker-build@^0.8"

# 部署至 Cloudflare（首次执行时需登录并选择目标账号）
cd server-rs
wrangler deploy
```

部署完成后，需为 Worker 配置以下两项密钥（可在 Cloudflare 控制台对应 Worker 的「设置 → 变量」中添加，或通过命令行完成）：

| 密钥       | 填写内容        | 说明                                           |
|----------|--------------|----------------------------------------------|
| `key`    | 自定义字符串       | 认证密钥，**请妥善保存**，客户端需使用同一密钥                     |
| `domain` | Worker 的域名   | 格式类似 `free-proxy.xxxx.workers.dev`，**注意不含端口号** |

### 第二步：启动客户端

```bash
cd client_tauri
pnpm tauri build   # 编译桌面端应用（支持 Windows / macOS / Linux）
# 编译完成后，安装并运行生成的应用程序
```

打开应用程序后，进入「代理设置」页面并填写以下信息：

- **域名**：与上述配置的 `domain` 保持一致（如 `free-proxy.xxxx.workers.dev`）；
- **认证密钥**：与上述配置的 `key` 保持一致；
- **协议**：默认使用 HTTP 即可，通常无需启用 HTTPS；
- **本地端口**：默认为 `8081`；
- **算法**：一般保持默认设置即可。

返回「仪表盘」页面，点击**启动代理**，等待显示「链路正常」及当前出口 IP 即表示配置成功。

### 第三步：导入订阅或设置代理

- 复制仪表盘中提供的订阅链接，通过代理客户端（如 Clash、v2rayN）导入使用；
- 或直接在操作系统或浏览器（借助 FoxyProxy 等插件）中配置 HTTP 代理。

### 第四步：安装信任 CA 证书（访问 HTTPS 站点必需）

若需通过客户端解密 HTTPS 流量，须将应用生成的本地证书添加至系统受信任的根证书列表：

- 打开客户端「CA 证书安装」页面，点击**一键安装**；
- **Linux 用户**需预先安装 `certutil` 工具，各发行版对应命令如下：

```bash
# Debian / Ubuntu
sudo apt install libnss3-tools
# Fedora / RHEL
sudo dnf install nss-tools
# Arch Linux
sudo pacman -S nss
```

> 该证书由本地设备生成并加密存储，仅对当前设备生效；更换设备后需重新导入。

---

## 四、优选 IP 使用说明

在「IP 优选测速」页面点击开始测速，应用将对一批候选 IP 地址依次执行两轮测试（连通性检测与 Worker 健康检查），并给出最优结果。将测速得出的最优 IP 填入「代理设置」页面的**优选 IP** 一栏（或直接应用测速结果），重新启动即可生效。在网络状况不佳的情况下，该操作通常能显著改善连接质量。

> 执行优选 IP 测速期间，请关闭 TUN 模式或其他代理工具，否则可能导致测速失败。

---

## 五、常见问题

**无法连接或无法正常上网？**
请按以下顺序逐项排查：① 域名与密钥是否与 Worker 端配置一致；② 本地端口是否被其他程序占用；③ 是否已完成 HTTPS 站点所需的 CA 证书信任设置；④ 「验证 Worker」是否提示连接正常。

**免费额度是否足够使用？**
Cloudflare Workers 免费版每日提供约 10 万次请求额度，可满足日常浏览、网页访问及社交媒体使用需求。若进行大流量下载，请留意用量情况，超出限额将导致当日服务暂停，次日额度自动恢复。

**部分网站无法访问？**
个别站点可能对代理访问设有限制，属正常现象。可尝试更换「优选 IP」或调整算法组合以改善访问情况。

**大文件上传经常失败或速度较慢？**
本地客户端与 Worker 之间对请求体采用流式加解密处理，而 Cloudflare 免费版对每次请求的 CPU 时间存在限制，超大文件累计的加解密开销可能触及该限制，导致传输中断。建议在上传前先对文件进行压缩，或升级至 Workers 付费版以提升处理上限。

**首次启动时链路状态异常？**
请前往「IP 优选测速」页面执行测速并选择合适的 IP。若未设置优选 IP，程序将回退至默认 DNS 解析方式，可能获取到受 DNS 污染影响的 IP 地址，或在 TLS 握手阶段遭遇基于 SNI 的连接阻断。

**如何查看日志或开启调试日志？**
客户端统一使用 `lib/log` 模块进行日志记录（可通过环境变量 `RUST_LOG` 控制日志级别及过滤条件，如 `RUST_LOG=debug`）。图形界面版本的日志写入应用数据目录下的 `logs/freeproxy.log` 文件（单文件上限 1MB，超出后自动轮转，不含 ANSI 颜色代码）；命令行版本的日志直接输出至终端标准错误流（带颜色标注）。Worker 端日志则输出至控制台。

---

## 六、工作原理

### 普通 HTTP 请求

普通 HTTP 请求经由 `/api/{version}/{target}` 路径完成转发：

```
浏览器
  │  （以 HTTP/HTTPS 明文形式交付本地客户端）
  ▼
本地客户端 ── 压缩 + 加密 ──▶ 免费 Worker ──▶ 目标网站
  ▲                                    │
  └────────── 加密返回 ────────────────┘
```

- 浏览器将流量转交给本地客户端处理；
- 客户端对数据进行**压缩与加密**后，经由 Worker 转发至目标网站；
- Worker 获取网站响应结果后，加密回传至客户端，客户端解密后写回浏览器；
- 客户端与 Worker 之间的通信基于 HTTP/2 协议。

### WebSocket 隧道

当浏览器发起 `ws://` / `wss://` 协议升级请求时，系统将自动切换至独立的 WebSocket 隧道（路径为 `/ws/{version}/{target}`）：

```
浏览器 ── ws/wss 升级请求 + RFC 6455 帧 ──▶ 本地客户端 ── 加密隧道消息 ──▶ Worker ──▶ 上游 WS 服务器
   ▲                                                                                   │
   └────────── 原始 WS 帧（101 响应头 / 数据帧 / close 帧）零解析回写 ───────────────┘
```

- **本地客户端**负责处理 RFC 6455 协议相关细节，包括帧解析、掩码处理及分片重组；浏览器发出的 Ping 帧由本地直接回复 Pong，不占用隧道带宽资源；
- **Worker 端**与上游服务器完成实际的 WebSocket 握手过程（包括依据客户端密钥计算 `Sec-WebSocket-Accept`），随后执行全双工数据转发，浏览器所接收到的仍为原始 WS 帧，text / binary / close 等语义完整保留；
- **连接保活机制**：隧道每隔 60 秒发送一次 Ping 帧（低于 Cloudflare 约 100 秒的空闲断开阈值），以避免长连接因空闲而被中断回收；
- 由于协议升级请求必须基于 HTTP/1.1（HTTP/2 不支持 Upgrade 语义），客户端与 Worker 之间的隧道通信通道已强制采用 HTTP/1.1 协议。

### 核心请求机制

系统通过 `worker::Fetch` 接口发送请求，而非 `worker::Socket`，因此不受相关限制，可正常连接使用 Cloudflare 服务的站点，从而实现更广泛的内容访问支持。

---

## 附录

### 目录结构

```
free-proxy/
├── package.json                  # 根 npm 脚本（server-dev/deploy, client-dev, test-lib, test-e2e）
├── deny.toml                     # cargo-deny 共享配置（四个 crate 共用）
├── .github/workflows/release.yml # tag 触发的发布 CI（桌面 + CLI + Worker zip）
├── lib/src/                      # 双端共享核心库——编译为 native 和 wasm32 两种目标
│   ├── aead.rs                   #   AEAD 加密：ChaCha20-Poly1305 / Ascon-AEAD128
│   ├── algo.rs                   #   压缩 × 加密组合协商 + URL 契约 /api/{version}/{target}
│   ├── base.rs                   #   base64 编解码
│   ├── client/                   #   仅客户端（feature "client"）：共享客户端逻辑
│   │   ├── mod.rs                #     ProxySettings / IDENTIFIER / DEFAULT_PORT 重导出
│   │   ├── config.rs             #     settings.json 读写（CLI 与 GUI 共享 app_data_dir）
│   │   ├── ca.rs                 #     本地 CA 证书管理
│   │   ├── speed.rs              #     优选 IP 测速编排
│   │   └── subscribe.rs          #     Clash / sing-box / base64 订阅导出
│   ├── compress.rs               #   zstd / lz4 压缩
│   ├── frames.rs                 #   二进制帧流协议：[4B 大端长度 | 负载]，零长帧 = 结束
│   ├── hash.rs                   #   sha1 / sha2 哈希
│   ├── http.rs                   #   httparse 头部解析 + UrlBuilder（零拷贝）
│   ├── kdf.rs                    #   HKDF 密钥派生
│   ├── lib.rs                    #   crate 根：feature 门控重导出 + 日志初始化
│   ├── log.rs                    #   统一日志宏（error!/warn!/info!/debug!/trace!）
│   ├── tool.rs                   #   derive_keys（auth_key+domain）、时间窗令牌认证、XOR 混淆
│   ├── ws.rs                     #   RFC 6455 帧 + WsTunnelMsg（客户端与 Worker 共用）
│   ├── proxy/                    #   仅客户端（feature "client"）：本地 HTTP 代理
│   │   ├── mod.rs                #     ProxyConfig / Shared / 代理生命周期
│   │   ├── connection.rs         #     连接分发（明文 HTTP vs CONNECT）
│   │   ├── body.rs               #     请求体边界解析
│   │   ├── client.rs             #     上游 reqwest 客户端（主/优选IP/WS HTTP1.1）
│   │   ├── tls.rs                #     MITM TLS：自签 CA + 每 SNI 叶子证书（moka 缓存）
│   │   └── core/                 #     转发引擎（serve 循环，EOS 保活）
│   │       ├── mod.rs            #       read_raw / RawRead（read_buf 零拷贝读循环）
│   │       ├── http.rs           #       HTTP 中继：头部帧 → 体泵 → 响应转发
│   │       └── ws.rs             #       WS 隧道客户端侧（RFC6455 解析/掩码/重组；
│   │                             #       上传/下载/写入任务 + ctrl/data 通道）
│   └── speed_test/               #   仅客户端：优选 IP 两阶段测速
│       ├── mod.rs                #     模块入口
│       ├── tcping.rs             #     阶段一：TCP 连接延迟探测
│       ├── health.rs             #     阶段二：Worker /health 检查
│       └── ip.rs                 #     Cloudflare IP 候选区间
├── server-rs/                    # Cloudflare Worker（Rust → wasm32，worker crate + axum）
│   ├── wrangler.toml             # worker 配置；[dev] port=80；勿动 compatibility_flags
│   ├── .dev.vars                 # gitignored 开发密钥（key/domain）
│   └── src/
│       ├── app.rs                #   axum 路由、Bearer 认证中间件（±30s 窗口）、路由表
│       ├── proxy_http.rs         #   POST /api/{version}/{target}：流式解密→请求→再加密
│       ├── proxy_ws.rs           #   GET /ws/{version}/{target}：上游 WS 握手 + 全双工中继
│       ├── subscribe.rs          #   GET /subscribe/{port}：Clash / sing-box / base64 订阅
│       └── lib.rs                #   Worker 入口
├── client_cli/src/               # CLI 客户端：main.rs（clap）、run.rs（代理循环）、speed.rs、
│                                 # health.rs、ca.rs（证书安装）、config.rs（settings.json）
├── client_tauri/                 # Tauri 2 + React 19 客户端（桌面 + Android）
│   ├── src/                      # React 前端：pages/（Dashboard、ProxySettings、SpeedTest、
│   │                             # CaCert、About）、components/{layout,ui}/、store/
│   └── src-tauri/                # Tauri 后端：commands/（proxy、speed、settings）、tray
└── lib_test/                     # E2E 全链路测试（cargo run，非 cargo test）
    └── src/
        ├── main.rs               #   测试入口
        ├── cs.rs                 #   全链路编排（Worker + 代理 + 目标站）
        └── test/                 #   HTTP / HTTPS 可达性与代理测试用例
```

### 开发命令

```bash
# 服务端本地开发（端口 80，密钥读取 server-rs/.dev.vars）
npm run server-dev
# 服务端部署
npm run server-deploy
# 客户端桌面开发
npm run client-dev
# 客户端 Android 开发
npm run client-android-dev
# 共享库单元测试
npm run test-lib
# 端到端集成测试
npm run test-e2e
```

### 安全模型

- 密钥经由 `auth_key + domain` 依次执行 SHA-256 哈希与 HKDF 派生，客户端与 Worker 各自独立推导出相同的密钥组，全程无需通过网络传输密钥本身；
- 每次请求所使用的快速认证令牌由 Ascon128、时间戳及随机数（nonce）共同构成，服务端仅接受时间窗口在 ±30 秒以内的有效令牌；
- 本地 CA 私钥采用设备唯一标识与随机盐值派生出的密钥进行加密存储，更换设备后需重新导入证书。

### 客户端界面

<div><img alt="" src="./image/screenshot-20260818-164651.png"></div>
<div><img alt="" src="./image/screenshot-20260818-164709.png"></div>

---

## 许可协议

本项目采用 **MIT OR Apache-2.0** 双重许可协议。

- [MIT License](LICENSE-MIT) — Copyright (c) 2026 ZEROLINGG
- [Apache License 2.0](LICENSE-APACHE) — Copyright 2026 ZEROLINGG