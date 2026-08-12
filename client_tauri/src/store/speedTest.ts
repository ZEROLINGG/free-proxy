import { create } from "zustand";
import { api, EVENTS, subscribeEvent } from "../lib/tauri";
import type {
  IpResult,
  ProxySettings,
  SpeedCancelledPayload,
  SpeedDonePayload,
  SpeedErrorPayload,
  SpeedPhase,
  SpeedPhasePayload,
  SpeedProgressPayload,
  SpeedTestOpts,
} from "../lib/types";
import { useUi } from "./ui";

export type { SpeedPhase };

interface SpeedState {
  phase: SpeedPhase;
  opts: SpeedTestOpts;
  rows: IpResult[];
  progress: { tested: number; total: number; rttMs: number | null } | null;
  bestIp: string | null;
  error: string | null;
  running: boolean;
  /** 当前会话代际号；事件 payload 的 gen 不一致时忽略（防旧会话残留事件） */
  gen: number | null;
  /** 最后一次收到事件的时间戳（watchdog 用） */
  lastEventAt: number;
  setOpts: (p: Partial<SpeedTestOpts>) => void;
  run: (s: ProxySettings) => Promise<void>;
  stop: () => Promise<void>;
  reset: () => void;
}

export const useSpeed = create<SpeedState>((set, get) => ({
  phase: "idle",
  // 默认值必须与 Rust 端 src-tauri/src/commands/speed.rs 的 SpeedTestOpts::default 保持一致
  // （跨语言契约，改动两端需同步）
  opts: {
    total: 8000,
    tcpingLimit: 96,
    tcpingTimeoutMs: 500,
    healthLimit: 32,
    healthTimeoutMs: 2000,
  },
  rows: [],
  progress: null,
  bestIp: null,
  error: null,
  running: false,
  gen: null,
  lastEventAt: 0,

  setOpts: (p) => set((s) => ({ opts: { ...s.opts, ...p } })),

  async run(s) {
    if (get().running) return;
    set({
      phase: "tcping",
      rows: [],
      progress: null,
      bestIp: null,
      error: null,
      running: true,
      gen: null,
      lastEventAt: Date.now(),
    });
    useUi.getState().setNavLocked(true);
    try {
      const gen = await api.speedTestStart(s, get().opts);
      set({ gen });
    } catch (e) {
      set({ phase: "error", error: String(e), running: false, gen: null });
      useUi.getState().setNavLocked(false);
      useUi.getState().toast(`测速启动失败：${e}`, "error");
    }
  },

  async stop() {
    if (!get().running) return;
    try {
      await api.speedTestCancel();
    } catch (e) {
      useUi.getState().toast(`取消失败：${e}`, "error");
    }
    // 乐观清空；后端 cancelled 事件到达后幂等收敛（gen 已置 null，旧事件被忽略）
    set({
      phase: "idle",
      rows: [],
      progress: null,
      bestIp: null,
      error: null,
      running: false,
      gen: null,
    });
    useUi.getState().setNavLocked(false);
    useUi.getState().toast("测速已停止");
  },

  reset: () =>
    set({
      phase: "idle",
      rows: [],
      progress: null,
      bestIp: null,
      error: null,
      running: false,
      gen: null,
    }),
}));

function isCurrent(p: { gen: number }): boolean {
  const s = useSpeed.getState();
  return s.running && s.gen === p.gen;
}

function touch() {
  useSpeed.setState({ lastEventAt: Date.now() });
}

subscribeEvent<SpeedPhasePayload>(EVENTS.speedPhase, (p) => {
  if (!isCurrent(p)) return;
  useSpeed.setState({ phase: p.phase });
  touch();
});

subscribeEvent<SpeedProgressPayload>(EVENTS.speedProgress, (p) => {
  if (!isCurrent(p)) return;
  useSpeed.setState({
    phase: p.phase,
    progress: { tested: p.tested, total: p.total, rttMs: p.rttMs },
  });
  touch();
});

subscribeEvent<SpeedDonePayload>(EVENTS.speedDone, (p) => {
  if (!isCurrent(p)) return;
  useSpeed.setState({
    phase: "done",
    rows: p.results,
    bestIp: p.bestIp,
    progress: null,
    running: false,
    gen: null,
  });
  useUi.getState().setNavLocked(false);
});

subscribeEvent<SpeedCancelledPayload>(EVENTS.speedCancelled, (p) => {
  if (!isCurrent(p)) return;
  useSpeed.setState({
    phase: "idle",
    rows: [],
    progress: null,
    bestIp: null,
    error: null,
    running: false,
    gen: null,
  });
  useUi.getState().setNavLocked(false);
});

subscribeEvent<SpeedErrorPayload>(EVENTS.speedError, (p) => {
  if (!isCurrent(p)) return;
  useSpeed.setState({ phase: "error", error: p.message, running: false, gen: null });
  useUi.getState().setNavLocked(false);
  useUi.getState().toast(`测速失败：${p.message}`, "error");
});
