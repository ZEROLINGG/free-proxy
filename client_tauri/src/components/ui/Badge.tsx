import clsx from "clsx";
import type { ReactNode } from "react";

type Variant = "neutral" | "accent" | "heat" | "live" | "error";

const styles: Record<Variant, string> = {
  neutral: "bg-[rgba(0,0,0,0.05)] text-ink2 dark:bg-white/10",
  accent: "bg-accent/10 text-accent-link",
  heat: "bg-heat-bg text-heat",
  live: "bg-live/15 text-[#1d9a45] dark:text-[#3ddc68]",
  error: "bg-error-bg text-error",
};

export function Badge({
  variant = "neutral",
  children,
  className,
}: {
  variant?: Variant;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={clsx(
        "inline-flex items-center gap-1.5 whitespace-nowrap rounded-pill px-2.5 py-1 text-[11.5px] font-medium",
        styles[variant],
        className,
      )}
    >
      {children}
    </span>
  );
}

/** 运行状态徽章：live 呼吸点 + 文本。 */
export function StatusDot({ on }: { on: boolean }) {
  return (
    <span
      className={clsx(
        "inline-flex items-center gap-2 text-[13px] font-medium",
        on ? "text-[#1d9a45] dark:text-[#3ddc68]" : "text-ink3",
      )}
    >
      <span className={clsx("h-2 w-2 rounded-full", on && "pulse-dot", !on && "bg-faint")} />
      {on ? "运行中" : "未运行"}
    </span>
  );
}
