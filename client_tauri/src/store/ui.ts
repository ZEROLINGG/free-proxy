import { create } from "zustand";
import { persist } from "zustand/middleware";

export type ViewId = "dashboard" | "proxy" | "speed" | "ca" | "about";
export type ThemeMode = "light" | "dark" | "auto";

export interface Toast {
  id: number;
  kind: "success" | "error" | "info";
  text: string;
}

interface UiState {
  view: ViewId;
  theme: ThemeMode;
  toasts: Toast[];
  /** 测速进行中锁住页面切换 */
  navLocked: boolean;
  navigate: (v: ViewId) => void;
  setTheme: (t: ThemeMode) => void;
  setNavLocked: (v: boolean) => void;
  toast: (text: string, kind?: Toast["kind"]) => void;
  dismiss: (id: number) => void;
}

export function applyTheme(mode: ThemeMode) {
  const dark =
    mode === "dark" ||
    (mode === "auto" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  document.documentElement.style.colorScheme = dark ? "dark" : "light";
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", dark ? "#161617" : "#f5f5f7");
}

let toastSeq = 0;

export const useUi = create<UiState>()(
  persist(
    (set, get) => ({
      view: "dashboard",
      theme: "auto",
      toasts: [],
      navLocked: false,
      navigate: (v) => {
        const { navLocked, view, toast } = get();
        if (navLocked && v !== view) {
          toast("测速进行中，请先停止后再切换页面", "info");
          return;
        }
        set({ view: v });
      },
      setTheme: (t) => {
        set({ theme: t });
        applyTheme(t);
      },
      setNavLocked: (v) => set({ navLocked: v }),
      toast: (text, kind = "info") => {
        const id = ++toastSeq;
        set((s) => ({ toasts: [...s.toasts.slice(-2), { id, kind, text }] }));
        setTimeout(() => get().dismiss(id), 3200);
      },
      dismiss: (id) =>
        set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
    }),
    { name: "free-proxy:ui", partialize: (s) => ({ theme: s.theme }) },
  ),
);
