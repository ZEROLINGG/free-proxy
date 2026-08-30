export type Compressor = "zstd" | "gzip" | "lz4" | "none";

export type Aead = "chacha20poly1305" | "ascon128";

export interface ProxySettings {
  domain: string;
  useHttps: boolean;
  authKey: string;
  localPort: number;
  compressor: Compressor;
  aead: Aead;
  prefIp: string | null;
}

export interface ProxyStatus {
  running: boolean;
  port: number;
  ip: string | null;
  compressor: string;
  aead: string;
}

export interface SpeedTestOpts {
  total: number;
  tcpingLimit: number;
  tcpingTimeoutMs: number;
  healthLimit: number;
  healthTimeoutMs: number;
}

export type SpeedPhase = "idle" | "tcping" | "health" | "done" | "error";

export interface IpResult {
  ip: string;
  rttMs: number;
}

export interface SpeedPhasePayload {
  gen: number;
  phase: "tcping" | "health";
}

export interface SpeedProgressPayload {
  gen: number;
  phase: "tcping" | "health";
  tested: number;
  total: number;
  rttMs: number | null;
}

export interface SpeedDonePayload {
  gen: number;
  results: IpResult[];
  bestIp: string | null;
  tested: number;
  healthy: number;
}

export interface SpeedErrorPayload {
  gen: number;
  message: string;
}

export interface SpeedCancelledPayload {
  gen: number;
}

export interface SpeedTestState {
  running: boolean;
}

export interface CaInfo {
  path: string;
  certPem: string;
  /** 本次加载是否自动重建了 CA（设备 uid 变化/密钥文件损坏），需重新导入证书 */
  rebuilt: boolean;
}

/** 代理可用性检测结果（proxy:availability 事件） */
export interface ProxyAvailability {
  ok: boolean;
  ip: string | null;
  latencyMs: number | null;
  error: string | null;
}

export const COMPRESSORS: { value: Compressor; label: string }[] = [
  { value: "zstd", label: "Zstandard" },
  { value: "gzip", label: "Gzip" },
  { value: "lz4", label: "LZ4" },
  { value: "none", label: "None（不压缩）" },
];

export const AEADS: { value: Aead; label: string }[] = [
  { value: "chacha20poly1305", label: "ChaCha20-Poly1305" },
  { value: "ascon128", label: "Ascon-AEAD128" },
];

// 以下默认值必须与 Rust 端 src-tauri/src/commands/settings.rs 的默认函数保持一致
// （跨语言契约，改动两端需同步）
export const DEFAULT_SETTINGS: ProxySettings = {
  domain: "",
  useHttps: false,
  authKey: "",
  localPort: 8001,
  compressor: "lz4",
  aead: "ascon128",
  prefIp: null,
};

/** 可校验失败的字段（Select/Segmented 字段永远合法，无需校验） */
export type ConfigErrorKey = "domain" | "localPort" | "authKey" | "prefIp";

export type SettingsErrors = Partial<Record<ConfigErrorKey, string>>;


export function validateSettings(s: ProxySettings): SettingsErrors {
  const e: SettingsErrors = {};
  const domain = s.domain.trim();
  if (!domain) {
    e.domain = "请填写 Worker 域名";
  } else if (hasExplicitPort(domain)) {
    // 与 Rust 端 Proxy::new 的校验对齐：域名携带端口会导致 token 派生不匹配（全链路 401）
    e.domain = "域名不能携带端口";
  }
  if (!Number.isInteger(s.localPort) || s.localPort < 1 || s.localPort > 65535)
    e.localPort = "端口范围 1–65535";
  if (!s.authKey) e.authKey = "请填写认证密钥";
  if (s.prefIp && s.prefIp.trim()) {
    const p = s.prefIp.trim();
    const ok =
      /^(\d{1,3}\.){3}\d{1,3}$/.test(p) &&
      p.split(".").every((o) => Number(o) <= 255);
    if (!ok) e.prefIp = "无效的 IPv4 地址";
  }
  return e;
}

/** 域名是否显式携带端口（与 Rust split_host_port 语义一致：裸 IPv6 不算）。 */
function hasExplicitPort(host: string): boolean {
  const h = host.trim();
  if (h.startsWith("[")) {
    const end = h.indexOf("]");
    if (end === -1) return false;
    return h.slice(end + 1).startsWith(":");
  }
  if ((h.match(/:/g) ?? []).length >= 2) return false; // 裸 IPv6，无端口
  return h.includes(":");
}

/** 配置完整性（由 settings 派生，UI 响应式使用）。 */
export interface ConfigCompleteness {
  ok: boolean;
  errors: SettingsErrors;
  /** 有错误的字段名（顺序稳定） */
  invalidFields: ConfigErrorKey[];
}

export function configCompleteness(s: ProxySettings): ConfigCompleteness {
  const errors = validateSettings(s);
  const invalidFields = Object.keys(errors) as ConfigErrorKey[];
  return { ok: invalidFields.length === 0, errors, invalidFields };
}

/** 字段 → 用户可读标签（完整性提示条等共用）。 */
export const FIELD_LABELS: Record<ConfigErrorKey, string> = {
  domain: "域名",
  localPort: "端口",
  authKey: "认证密钥",
  prefIp: "优选 IP",
};

export function isAead(v: string): v is Aead {
  return AEADS.some((a) => a.value === v);
}

export function isCompressor(v: string): v is Compressor {
  return COMPRESSORS.some((c) => c.value === v);
}

export function subscribeUrl(domain: string, useHttps: boolean, port: number): string {
  const d = domain.trim();
  if (!d) throw new Error("domain 不能为空");
  const scheme = useHttps ? "https" : "http";
  return `${scheme}://${d}/subscribe/${port}`;
}
