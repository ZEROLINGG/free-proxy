# free-proxy — 零成本私人独享代理

> 不用买 VPS、不用买域名、不用备案，只用 **Cloudflare 免费的 Worker**，就能部署一个私人且几乎无限流量的代理节点。

---

## 一、这是什么？

`free-proxy` 是一个「客户端 + 免费中转服务」组成的私人代理方案：

- **服务端**跑在 Cloudflare Worker 上——这是 Cloudflare 的**免费**服务，按免费额度提供日常请求量，个人上网绰绰有余；
- **客户端**是桌面 / 手机上的一个小应用，负责加密、压缩、加速，并把流量交给你的 Worker 转发。

<div><img alt="" src="./image/screenshot-20260821-194431.png"></div>
<div><img alt="" src="./image/screenshot-20260818-164913.png"></div>

### 它能给你带来什么

- **免费**：全部依赖 Cloudflare 免费额度，零月租、零维护成本；
- **独享**：节点只属于你，认证密钥只在你和设备两端；
- **省流量**：内置压缩（zstd / gzip / lz4），相同内容传得少、跑得快；
- **更安全**：多种加密算法可选（AES-GCM / ChaCha20-Poly1305 等），传输内容被加密；
- **自动挑最快的路**：内置「优选 IP」测速工具，帮你找到与 Worker 之间最快的网络路径；
- **全家桶通用**：客户端可一键导出订阅链接，Clash / sing-box / v2rayN 等工具都能用；
- **WebSocket支持**：内置 WebSocket 隧道，聊天 / 推送 / 在线协作等基于 `ws://` / `wss://` 的应用同样走加密代理；
- **跨平台**：桌面（Windows / macOS / Linux）+ 手机（Android和iOS 实验性支持）。

---

## 二、你需要准备什么

1. 一个 **Cloudflare 账号**（免费注册）；
2. 一台**电脑**。
3. 电脑上装好：**Node.js**、**Rust 工具链**、**pnpm**。


---

## 三、快速上手

### 第 1 步：部署你的免费 Worker

```bash
# 安装构建工具（只需一次）
cargo install -q "worker-build@^0.8"

# 部署到 Cloudflare（首次会让你登录并选择账号）
cd server-rs
wrangler deploy
```

部署完成后，给 Worker 配置两个密钥（在 Cloudflare 控制台对应 Worker 的「设置 → 变量」里添加，或命令行执行）：

| 密钥       | 填什么          | 说明                                            |
|----------|--------------|-----------------------------------------------|
| `key`    | 你自己随便编一串字符   | 认证密钥，**请记好**，客户端要用同一个                         |
| `domain` | 你的 Worker 域名 | 形如 `free-proxy.xxxx.workers.dev`，**注意不要带端口号** |

### 第 2 步：启动客户端

```bash
cd client_tauri
pnpm tauri build   # 桌面端（Windows / macOS / Linux）
# 然后安装运行构建的程序
```

打开应用后，进入「代理」页填写：

- **域名**：和上面 `domain` 一致（如 `free-proxy.xxxx.workers.dev`）；
- **认证密钥**：和上面 `key` 一致；
- **协议**：默认 HTTP 即可（走 HTTPS 一般没必要）；
- **本地端口**：默认 `8081`；
- **算法**：一般默认即可。

回到「仪表盘」，点击**启动代理**，等待「链路正常」+ 显示出口 IP


### 第 3 步：导入订阅或设置代理
- 复制仪表板中的订阅链接，使用你你的代理客户端（如clash,v2rayN）导入。
- 或者在系统或浏览器（通过FoxyProxy或类似插件）直接设置http代理。

### 第 4 步：信任 CA 证书（访问 HTTPS 网站必需）

客户端要能解密 HTTPS，需要把应用生成的本地证书信任为系统根证书：

- 打开客户端的「CA 证书安装」页，点击**一键安装**；
- **Linux 用户**需要先安装 `certutil`，各发行版命令：

```bash
# Debian / Ubuntu
sudo apt install libnss3-tools
# Fedora / RHEL
sudo dnf install nss-tools
# Arch Linux
sudo pacman -S nss
```

> 该证书由你的设备本地生成并加密保存，只影响你自己的设备；换设备需重新导入。

---

## 四、优选 IP


在「IP 优选测速」页点击开始，应用会自动对一批候选 IP 做两轮测速（连通性 + Worker 健康检查），并给出最优结果。把最优 IP 填进「代理」页的**优选 IP** 栏（或直接一键应用），下次启动即生效——网络不好时这一步往往立竿见影。

> 优选ip时请关闭tun或其他代理，否则将导致优选失败

---

## 五、常见问题（FAQ）

**为什么连不上 / 无法上网？**
按顺序排查：① 域名和密钥是否与 Worker 一致；② 本地端口是否被占用；③ HTTPS 站点是否已信任 CA 证书；④ 「验证 Worker」是否提示连接正常。

**免费额度够用吗？**
Cloudflare Workers 免费版每天提供约 10 万次请求额度，个人日常浏览、看网页、刷社交足够。大流量下载请留意用量，超限会暂停当天服务，次日自动恢复。

**为什么有的网站打不开？**
部分站点对代理访问有限制，属正常现象；另外可尝试切换「优选 IP」或更换算法组合。

**大文件上传总是失败 / 很慢？**
本地客户端与 Worker 对请求体做流式加解密，Cloudflare 免费版每个请求的 CPU 时间有限，超大请求体的累计解密/解压开销可能触顶导致传输中断。建议先压缩文件再上传，或升级 Workers Paid 提升上限。

**为什么首次启动链路异常？**
需要去「IP 优选测速」页点测速选择一个ip，否则无优选ip会回退到DNS解析，将导致获得受DNS污染的ip或tls握手时受到sni阻断。

**在哪里看日志 / 怎么开启 debug 日志？**
客户端统一使用 `lib/log` 日志（`RUST_LOG` 控制等级与过滤，如 `RUST_LOG=debug`）。GUI 日志写入应用数据目录的 `logs/freeproxy.log`（1MB 轮转，ANSI 关闭）；命令行客户端输出到终端 stderr（带颜色）。Worker 端日志打印到控制台。

---

## 六、工作原理

### 普通Http请求

普通 HTTP 请求走 `/api/{version}/{target}` 转发。

```
浏览器
  │  （HTTP/HTTPS 明文交给本地客户端）
  ▼
本地客户端 ── 压缩 + 加密 ──▶ 免费 Worker ──▶ 目标网站
  ▲                                    │
  └────────── 加密返回 ────────────────┘
```

- 浏览器把流量交给本地客户端；
- 客户端**压缩 + 加密**后，通过Worker转发到目标网站；
- Worker 拿到网站响应，再加密传回客户端解密，写回浏览器；
- 通过http2请求worker。

### WebSocket 隧道

浏览器发起 `ws://` / `wss://` 升级请求时，会自动切换到独立的 WebSocket 隧道（`/ws/{version}/{target}`）：

```
浏览器 ── ws/wss 升级请求 + RFC 6455 帧 ──▶ 本地客户端 ── 加密隧道消息 ──▶ Worker ──▶ 上游 WS 服务器
   ▲                                                                                   │
   └────────── 原始 WS 帧（101 响应头 / 数据帧 / close 帧）零解析回写 ───────────────┘
```

- **本地客户端**负责 RFC 6455 协议细节：帧解析、掩码解掩码、分片重组；浏览器的 Ping 帧本地直接回 Pong，不占用隧道带宽；
- **Worker 端**与上游完成真正的 WebSocket 握手（含按客户端 key 计算 `Sec-WebSocket-Accept`），之后全双工转发，浏览器收到的仍是原始 WS 帧，text / binary / close 语义完整保留；
- **保活**：隧道侧每 60 秒发一次 Ping（低于 Cloudflare 约 100 秒的空闲断开阈值），避免长连接被空闲回收；
- 升级请求必须走 HTTP/1.1（HTTP/2 不支持 Upgrade 语义），客户端连接 Worker 的隧道通道已强制 HTTP/1.1。

### 请求核心
通过worker::Fetch 接口发送请求，而不是worker::Socket，不受限制，可以连接使用了cf服务的站点，能够解锁更多内容。


---

## 附录

### 目录结构

```
free-proxy/
├── lib/                  # 双端共享核心库（客户端 / Worker 共用同一份实现）
│   └── src/
│       ├── algo.rs       # 算法分发：压缩 × 加密组合与 URL 契约
│       ├── frames.rs     # 私有二进制帧流协议（[4B 长度 | 负载]，零长帧 = 结束）
│       ├── http.rs       # HTTP 头解析（httparse，零拷贝）
│       ├── aead.rs       # AES-GCM / AES-GCM-SIV / ChaCha20-Poly1305…
│       ├── compress.rs   # zstd / gzip / lz4
│       ├── kdf.rs        # PBKDF2 / scrypt / HKDF
│       ├── tool.rs       # 密钥派生、时间窗令牌、异或混淆
│       ├── ws.rs         # WebSocket 协议层：RFC 6455 帧解析 / 分片重组 / 隧道消息（WsTunnelMsg）
│       └── proxy/        # 客户端本地代理 + MITM TLS（tls.rs）+ WS 隧道（ws.rs）
│       └── speed_test/   # tcping / health 两阶段优选 IP 测速
├── server-rs/            # Cloudflare Worker（Rust 编译到 wasm32）
│   ├── wrangler.toml
│   └── src/              # /api/{version}/{target} HTTP 代理、/ws/{version}/{target} WS 隧道、/subscribe 订阅、/health
├── client_cli/           # 命令行客户端
└── client_tauri/         # Tauri 2 + React 19 客户端（桌面 + Android）
    ├── src/              # 前端页面与状态（Dashboard / 代理 / 测速 / CA）
    └── src-tauri/        # 本地代理、CA 安装、测速等 Tauri 命令
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

# 共享库单元测试（含大量客户端↔服务端契约 / 往返测试）
cargo test -p lib
```

### 安全模型

- 密钥由 `auth_key + domain` 经 PBKDF2/HKDF 派生，客户端与 Worker 两端独立推导出同一组密钥，无需网络传输密钥；
- 每次请求的快速认证令牌由Aes128Gcm + 时间戳 + nonce 组成，服务端仅接受 ±30 秒内的令牌；
- 本地 CA 私钥使用设备唯一标识 + 随机盐派生密钥加密存储，换设备需重新导入证书。




### 客户端界面

<div><img alt="" src="./image/screenshot-20260818-164651.png"></div>
<div><img alt="" src="./image/screenshot-20260818-164709.png"></div>

---

## 许可

本项目采用 **MIT OR Apache-2.0** 双许可。

- [MIT License](LICENSE-MIT) — Copyright (c) 2026 ZEROLINGG
- [Apache License 2.0](LICENSE-APACHE) — Copyright 2026 ZEROLINGG

贡献即视为同意以相同双许可分发。

