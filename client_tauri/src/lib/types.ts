export type Compressor = "zstd" | "gzip" | "lz4" | "none";

export type Aead =
  | "aes128gcm"
  | "aes256gcm"
  | "aes128gcmsiv"
  | "aes256gcmsiv"
  | "chacha20poly1305"
  | "xchacha20poly1305"
  | "none";

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
  { value: "aes128gcm", label: "AES-128-GCM" },
  { value: "aes256gcm", label: "AES-256-GCM" },
  { value: "aes128gcmsiv", label: "AES-128-GCM-SIV" },
  { value: "aes256gcmsiv", label: "AES-256-GCM-SIV" },
  { value: "chacha20poly1305", label: "ChaCha20-Poly1305" },
  { value: "xchacha20poly1305", label: "XChaCha20-Poly1305" },
  { value: "none", label: "None（异或混淆）" },
];

// 以下默认值必须与 Rust 端 src-tauri/src/commands/settings.rs 的默认函数保持一致
// （跨语言契约，改动两端需同步）
export const DEFAULT_SETTINGS: ProxySettings = {
  domain: "",
  useHttps: false,
  authKey: "",
  localPort: 8080,
  compressor: "zstd",
  aead: "aes128gcm",
  prefIp: null,
};

export type SettingsErrors = Partial<Record<keyof ProxySettings, string>>;


export function validateSettings(s: ProxySettings): SettingsErrors {
  const e: SettingsErrors = {};
  const domain = s.domain.trim();
  if (!domain) e.domain = "请填写 Worker 域名";
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

export function isAead(v: string): v is Aead {
  return AEADS.some((a) => a.value === v);
}

export function isCompressor(v: string): v is Compressor {
  return COMPRESSORS.some((c) => c.value === v);
}
