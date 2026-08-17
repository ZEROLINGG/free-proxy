import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect } from "react";
import type {
  CaInfo,
  ProxySettings,
  ProxyStatus,
  SpeedTestOpts,
  SpeedTestState,
} from "./types";

export const api = {
  loadSettings: () => invoke<ProxySettings>("load_settings"),
  saveSettings: (s: ProxySettings) => invoke<void>("save_settings", { s }),
  proxyStart: (s: ProxySettings) => invoke<number>("proxy_start", { s }),
  proxyStop: () => invoke<void>("proxy_stop"),
  proxyStatus: () => invoke<ProxyStatus>("proxy_status"),
  proxySetAead: (aead: string) => invoke<void>("proxy_set_aead", { aead }),
  proxySetCompressor: (compressor: string) =>
    invoke<void>("proxy_set_compressor", { compressor }),
  proxySetIp: (ip: string | null) => invoke<void>("proxy_set_ip", { ip }),
  proxyCheckAvailability: () => invoke<void>("proxy_check_availability"),
  openCaDir: () => invoke<void>("open_ca_dir"),
  caInfo: () => invoke<CaInfo>("ca_info"),
  installCa: () => invoke<void>("install_ca"),
  speedTestStart: (s: ProxySettings, opts: SpeedTestOpts) =>
    invoke<number>("speed_test_start", { s, opts }),
  speedTestCancel: () => invoke<void>("speed_test_cancel"),
  speedTestState: () => invoke<SpeedTestState>("speed_test_state"),
  workerHealth: (s: ProxySettings) => invoke<boolean>("worker_health", { s }),
};

export async function appVersion(): Promise<string | null> {
  try {
    return await getVersion();
  } catch {
    return null;
  }
}

export const EVENTS = {
  proxyStatus: "proxy:status",
  proxyAvailability: "proxy:availability",
  speedPhase: "speed-test:phase",
  speedProgress: "speed-test:progress",
  speedDone: "speed-test:done",
  speedError: "speed-test:error",
  speedCancelled: "speed-test:cancelled",
} as const;

export type ProgressHandler<T> = (payload: T) => void;

/** 订阅 Tauri 事件；组件卸载时自动清理。非 Tauri 环境（纯浏览器预览）静默失败。 */
export function useTauriEvent<T>(
  event: string,
  handler: ProgressHandler<T>,
) {
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    listen<T>(event, (e) => handler(e.payload))
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [event, handler]);
}

/** 全局单例事件订阅（store 层使用，不随组件生命周期销毁）。
 *  按事件名去重：dev 热重载重复执行模块时不会叠加监听器。 */
const subscribed = new Set<string>();

export function subscribeEvent<T>(
  event: string,
  handler: ProgressHandler<T>,
): () => void {
  if (subscribed.has(event)) return () => {};
  subscribed.add(event);
  let unlisten: UnlistenFn | null = null;
  listen<T>(event, (e) => handler(e.payload))
    .then((fn) => (unlisten = fn))
    .catch(() => {});
  return () => {
    subscribed.delete(event);
    unlisten?.();
  };
}
