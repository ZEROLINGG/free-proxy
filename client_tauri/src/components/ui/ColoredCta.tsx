import { Square, Zap } from "lucide-react";
import clsx from "clsx";
import { Spinner } from "./Spinner";

interface Props {
  running: boolean;
  busy: boolean;
  onClick: () => void;
}

/**
 * 仪表盘大开关（渐变 + 光球 + 玻璃徽章）
 */
export function ColoredCta({ running, busy, onClick }: Props) {
  return (
      <section
          className={clsx(
              "group relative overflow-hidden rounded-hero transition-all duration-500",
              running
                  ? "border-2 border-live/30 bg-surface shadow-panel"
                  : "border border-white/10 shadow-cta " +
                  // Light 模式：300/400 系列 - 更亮更通透
                  "bg-linear-to-br from-emerald-300 via-teal-400 to-cyan-400 " +
                  // Dark 模式：保持原有配色
                  "dark:from-emerald-400 dark:via-teal-400 dark:to-cyan-500",
          )}
      >
        {/* ═══════════════════════════════════════════════════════════
          未运行状态 - 炫彩科技风
          ═══════════════════════════════════════════════════════════ */}
        {!running && (
            <>
              {/* 网格背景层 */}
              <div
                  aria-hidden
                  className="absolute inset-0 opacity-[0.21]"
                  style={{
                    backgroundImage: `
                linear-gradient(rgba(255,255,255,0.6) 1px, transparent 1px),
                linear-gradient(90deg, rgba(255,255,255,0.6) 1px, transparent 1px)
              `,
                    backgroundSize: '28px 28px',
                    backgroundPosition: 'center center',
                  }}
              />

              {/* 对角线条纹 */}
              <div
                  aria-hidden
                  className="absolute inset-0 opacity-[0.08]"
                  style={{
                    backgroundImage: `repeating-linear-gradient(
                45deg,
                transparent,
                transparent 20px,
                rgba(255,255,255,0.7) 20px,
                rgba(255,255,255,0.7) 21px
              )`,
                  }}
              />

              {/* 主光球 - Light 50% 透明度（Dark 保持 30%） */}
              <div
                  aria-hidden
                  className="absolute -right-16 -top-28 h-80 w-80 animate-breath-slow rounded-full bg-white/50 blur-[70px] dark:bg-white/30"
              />

              {/* 次光球 - Light 50% 青绿光源（Dark 40%） */}
              <div
                  aria-hidden
                  className="absolute -bottom-24 -left-20 h-72 w-72 animate-breath-delayed rounded-full bg-emerald-300/50 blur-[65px] dark:bg-emerald-300/40"
              />

              {/* 漂浮光球 - Light 提升亮度 */}
              <div
                  aria-hidden
                  className="absolute right-[20%] top-[35%] h-56 w-56 animate-float rounded-full bg-cyan-200/40 blur-[50px] dark:bg-cyan-200/35"
              />

              {/* 顶部高光线 - Light 强化发光 + 阴影 */}
              <div
                  aria-hidden
                  className={clsx(
                      "absolute inset-x-0 top-0 h-0.5 bg-linear-to-r from-transparent to-transparent",
                      "via-white/80 shadow-[0_1px_8px_rgba(255,255,255,0.6)]",
                      "dark:via-white/60 dark:shadow-none"
                  )}
              />

              {/* 边缘内发光 - Light 增强质感 */}
              <div
                  aria-hidden
                  className={clsx(
                      "absolute inset-0 rounded-hero",
                      "shadow-[inset_0_1px_1px_rgba(255,255,255,0.4),inset_0_-1px_1px_rgba(255,255,255,0.15)]",
                      "dark:shadow-[inset_0_1px_1px_rgba(255,255,255,0.2)]"
                  )}
              />

              {/* 底部微光线 - Light 提升亮度 */}
              <div
                  aria-hidden
                  className="absolute inset-x-0 bottom-0 h-[1.5px] bg-linear-to-r from-transparent via-white/40 to-transparent dark:via-white/25"
              />

              {/* 装饰闪电 - 左上（Light 30% + 发光） */}
              <div
                  aria-hidden
                  className={clsx(
                      "absolute left-7 top-7 animate-pulse-slower",
                      "opacity-30 drop-shadow-[0_0_8px_rgba(255,255,255,0.8)]",
                      "dark:opacity-20 dark:drop-shadow-none"
                  )}
              >
                <Zap
                    size={18}
                    className="text-white"
                    strokeWidth={2.5}
                    fill="rgba(255,255,255,0.5)"
                />
              </div>

              {/* 装饰闪电 - 右下（Light 25% + 发光） */}
              <div
                  aria-hidden
                  className={clsx(
                      "absolute bottom-7 right-9 rotate-12 animate-pulse-slow",
                      "opacity-25 drop-shadow-[0_0_6px_rgba(255,255,255,0.7)]",
                      "dark:opacity-15 dark:drop-shadow-none"
                  )}
              >
                <Zap
                    size={22}
                    className="text-white"
                    strokeWidth={2.5}
                    fill="rgba(255,255,255,0.4)"
                />
              </div>

              {/* 彩色光点粒子群 - Light 使用彩色 + 强阴影 */}
              <div
                  aria-hidden
                  className={clsx(
                      "absolute left-[22%] top-[28%] h-2 w-2 animate-pulse-slow rounded-full",
                      "bg-cyan-200 shadow-[0_0_12px_rgba(6,182,212,0.9)]",
                      "dark:bg-white/70 dark:shadow-[0_0_8px_rgba(255,255,255,0.8)]"
                  )}
              />
              <div
                  aria-hidden
                  className={clsx(
                      "absolute right-[28%] top-[65%] h-1.5 w-1.5 animate-pulse-slower rounded-full",
                      "bg-emerald-300 shadow-[0_0_10px_rgba(16,185,129,0.8)]",
                      "dark:bg-white/60 dark:shadow-[0_0_6px_rgba(255,255,255,0.7)]"
                  )}
              />
              <div
                  aria-hidden
                  className={clsx(
                      "absolute left-[68%] bottom-[35%] h-1 w-1 animate-pulse rounded-full",
                      "bg-teal-200 shadow-[0_0_8px_rgba(20,184,166,0.7)]",
                      "dark:bg-white/50 dark:shadow-[0_0_4px_rgba(255,255,255,0.6)]"
                  )}
              />
              <div
                  aria-hidden
                  className="absolute left-[45%] top-[18%] h-1 w-1 animate-pulse-slower rounded-full bg-white/60 dark:bg-white/45"
              />
            </>
        )}

        {/* ═══════════════════════════════════════════════════════════
          运行中状态 - 绿色脉动光环
          ═══════════════════════════════════════════════════════════ */}
        {running && (
            <>
              {/* 外层脉动光晕 */}
              <div
                  aria-hidden
                  className="absolute -inset-1 -z-10 animate-pulse-glow rounded-hero bg-live/25 blur-2xl"
              />

              {/* 内层微光 */}
              <div
                  aria-hidden
                  className="absolute right-10 top-10 h-40 w-40 animate-breath-slow rounded-full bg-live/8 blur-3xl"
              />

            </>
        )}

        {/* ═══════════════════════════════════════════════════════════
          主按钮区域
          ═══════════════════════════════════════════════════════════ */}
        <button
            type="button"
            disabled={busy}
            onClick={onClick}
            className={clsx(
                "press relative z-10 flex w-full select-none flex-col items-center justify-center gap-2.5 py-11",
                "transition-all duration-300",
                "disabled:pointer-events-none disabled:opacity-70",
                !running && [
                  "hover:scale-[1.02]",
                  "hover:shadow-[0_0_40px_rgba(20,184,166,0.4),0_20px_60px_rgba(0,113,227,0.3)]"
                ],
            )}
        >
          {/* 图标区 - 带光晕效果 */}
          <div className="relative">
            {!running && !busy && (
                <div
                    aria-hidden
                    className="absolute -inset-3 -z-10 animate-pulse-slow rounded-full bg-white/50 blur-2xl dark:bg-white/40"
                />
            )}

            {busy ? (
                <Spinner
                    className={clsx(
                        "h-10 w-10",
                        running ? "text-ink" : "text-white drop-shadow-lg"
                    )}
                />
            ) : running ? (
                <div className="relative">
                  <Square
                      size={34}
                      className="fill-ink text-ink transition-transform group-hover:scale-105"
                      strokeWidth={2.5}
                  />
                </div>
            ) : (
                <Zap
                    size={38}
                    className={clsx(
                        "fill-white/25 text-white transition-transform group-hover:scale-110",
                        "drop-shadow-[0_6px_16px_rgba(255,255,255,0.6)]",
                        "dark:drop-shadow-[0_4px_12px_rgba(255,255,255,0.5)]"
                    )}
                    strokeWidth={2.8}
                />
            )}
          </div>

          {/* 主标题 - Light 增强阴影 */}
          <span
              className={clsx(
                  "mt-0.5 text-[21px] font-bold leading-tight tracking-tight transition-all",
                  running
                      ? "text-ink"
                      : "text-white " +
                      "drop-shadow-[0_3px_12px_rgba(0,0,0,0.35)] " +
                      "dark:drop-shadow-[0_2px_8px_rgba(0,0,0,0.2)]",
              )}
          >
          {busy ? (running ? "正在停止…" : "正在启动…") : running ? "停止代理" : "开始代理"}
        </span>

          {/* 副标题 - Light 增强对比 */}
          <span
              className={clsx(
                  "text-[13.5px] font-medium transition-all",
                  running
                      ? "text-ink3"
                      : "text-white/95 drop-shadow-[0_2px_6px_rgba(0,0,0,0.25)] " +
                      "dark:text-white/92 dark:drop-shadow-[0_1px_4px_rgba(0,0,0,0.15)]",
              )}
          >
          {running ? "本地代理运行中，点击停止" : "启动本地代理 127.0.0.1"}
        </span>
        </button>
      </section>
  );
}