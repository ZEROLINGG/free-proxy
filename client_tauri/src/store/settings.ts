import { create } from "zustand";
import { isTauri } from "@tauri-apps/api/core";
import { api } from "../lib/tauri";
import {
  configCompleteness,
  DEFAULT_SETTINGS,
  type ConfigCompleteness,
  type ProxySettings,
} from "../lib/types";
import { useProxy } from "./proxy";
import { useUi } from "./ui";

interface SettingsState {
  settings: ProxySettings;
  saved: ProxySettings | null;
  loading: boolean;
  /** 配置完整性（由 settings 派生，patch/load 时同步更新） */
  completeness: ConfigCompleteness;
  load: () => Promise<void>;
  patch: (p: Partial<ProxySettings>) => void;
  save: () => Promise<void>;
  /** 应用优选 IP：写入设置并持久化；代理运行中则同时热切换。 */
  applyIp: (ip: string | null) => Promise<boolean>;
}

export const useSettings = create<SettingsState>((set, get) => ({
  settings: DEFAULT_SETTINGS,
  saved: null,
  loading: true,
  completeness: configCompleteness(DEFAULT_SETTINGS),

  async load() {
    try {
      const s = await api.loadSettings();
      set({
        settings: { ...DEFAULT_SETTINGS, ...s },
        saved: s,
        loading: false,
        completeness: configCompleteness({ ...DEFAULT_SETTINGS, ...s }),
      });
    } catch {
      set({
        settings: DEFAULT_SETTINGS,
        saved: DEFAULT_SETTINGS,
        loading: false,
        completeness: configCompleteness(DEFAULT_SETTINGS),
      });
      // 文件损坏才走到这里（文件缺失已由 Rust 端返回默认值）；浏览器预览环境静默
      if (isTauri()) {
        useUi.getState().toast("设置读取失败，已恢复默认配置", "error");
      }
    }
  },

  patch: (p) =>
    set((s) => {
      const next = { ...s.settings, ...p };
      // 连接类配置变化后，旧的 Worker 健康检查结果不再可信
      const connKeys: (keyof ProxySettings)[] = [
        "domain",
        "useHttps",
        "authKey",
      ];
      const connChanged = connKeys.some((k) => s.settings[k] !== next[k]);
      if (connChanged) useProxy.setState({ health: null });
      return { settings: next, completeness: configCompleteness(next) };
    }),

  async save() {
    const s = get().settings;
    await api.saveSettings(s);
    set({ saved: s });
  },

  async applyIp(ip: string | null) {
    const trimmed = ip?.trim() || null;
    const { settings } = get();
    if (settings.prefIp === trimmed) return true;
    const next = { ...settings, prefIp: trimmed };
    set({ settings: next, completeness: configCompleteness(next) });
    try {
      await api.saveSettings(next);
      set({ saved: next });
    } catch (e) {
      useUi.getState().toast(`保存设置失败：${e}`, "error");
      return false;
    }
    const proxy = useProxy.getState();
    if (proxy.status.running) {
      try {
        // 走 store action：本地即时同步 status（后端 emit 事件为第二通道）
        await proxy.setIp(trimmed);
        useUi.getState().toast(
          trimmed ? `已热切换优选 IP ${trimmed}` : "已清除优选 IP，回退 DNS 解析",
          "success",
        );
      } catch (e) {
        useUi.getState().toast(`热切换 IP 失败：${e}`, "error");
        return false;
      }
    } else {
      useUi.getState().toast(
        trimmed ? `已应用优选 IP ${trimmed}（下次启动生效）` : "已清除优选 IP",
        "success",
      );
    }
    return true;
  },
}));
