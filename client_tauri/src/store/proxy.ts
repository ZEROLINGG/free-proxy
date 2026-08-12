import { create } from "zustand";
import { api, EVENTS, subscribeEvent } from "../lib/tauri";
import type {
  Aead,
  Compressor,
  ProxyAvailability,
  ProxySettings,
  ProxyStatus,
} from "../lib/types";
import { useUi } from "./ui";

interface ProxyState {
  status: ProxyStatus;
  busy: boolean;
  health: boolean | null;
  healthBusy: boolean;
  availability: ProxyAvailability | null;
  availabilityBusy: boolean;
  refresh: () => Promise<void>;
  start: (s: ProxySettings) => Promise<void>;
  stop: () => Promise<void>;
  setAead: (a: Aead) => Promise<void>;
  setCompressor: (c: Compressor) => Promise<void>;
  setIp: (ip: string | null) => Promise<void>;
  checkHealth: (s: ProxySettings) => Promise<void>;
  checkAvailability: () => Promise<void>;
}

export const useProxy = create<ProxyState>((set) => ({
  status: {
    running: false,
    port: 0,
    ip: null,
    compressor: "lz4",
    aead: "aes128gcm",
  },
  busy: false,
  health: null,
  healthBusy: false,
  availability: null,
  availabilityBusy: false,

  async refresh() {
    try {
      const status = await api.proxyStatus();
      set({ status, busy: false });
    } catch {
      /* 非 Tauri 环境 */
    }
  },

  async start(s) {
    set({ busy: true, availability: null, availabilityBusy: false });
    try {
      await api.proxyStart(s);
      await useProxy.getState().refresh();
      useUi.getState().toast("代理已启动", "success");
    } catch (e) {
      set({ busy: false });
      throw e;
    }
  },

  async stop() {
    set({ busy: true, availability: null, availabilityBusy: false });
    try {
      await api.proxyStop();
      await useProxy.getState().refresh();
      useUi.getState().toast("代理已停止");
    } catch (e) {
      set({ busy: false });
      throw e;
    }
  },

  async setAead(a) {
    await api.proxySetAead(a);
    set((s) => ({ status: { ...s.status, aead: a } }));
  },

  async setCompressor(c) {
    await api.proxySetCompressor(c);
    set((s) => ({ status: { ...s.status, compressor: c } }));
  },

  async setIp(ip) {
    await api.proxySetIp(ip);
    set((s) => ({ status: { ...s.status, ip: ip } }));
  },

  async checkHealth(s) {
    set({ healthBusy: true });
    try {
      const ok = await api.workerHealth(s);
      set({ health: ok, healthBusy: false });
      useUi
        .getState()
        .toast(ok ? "Worker 连接正常" : "Worker 健康检查未通过", ok ? "success" : "error");
    } catch (e) {
      set({ health: false, healthBusy: false });
      useUi.getState().toast(`健康检查失败：${e}`, "error");
    }
  },

  async checkAvailability() {
    set({ availabilityBusy: true });
    try {
      await api.proxyCheckAvailability();
    } catch (e) {
      useUi.getState().toast(`可用性检测失败：${e}`, "error");
    } finally {
      set({ availabilityBusy: false });
    }
  },
}));

subscribeEvent<ProxyStatus>(EVENTS.proxyStatus, (payload) => {
  useProxy.setState({ status: payload, busy: false });
});

subscribeEvent<ProxyAvailability>(EVENTS.proxyAvailability, (payload) => {
  // 代理已停止后到达的旧检测结果直接忽略
  if (!useProxy.getState().status.running) return;
  useProxy.setState({ availability: payload, availabilityBusy: false });
});
