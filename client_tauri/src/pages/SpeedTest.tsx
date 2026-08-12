import { ChevronDown, ChevronUp, Play, Square } from "lucide-react";
import clsx from "clsx";
import { useEffect, useState } from "react";
import { useSettings } from "../store/settings";
import { useSpeed, type SpeedPhase } from "../store/speedTest";
import { useUi } from "../store/ui";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Input } from "../components/ui/Input";
import { ListRow, Panel } from "../components/ui/Panel";
import { Progress } from "../components/ui/Progress";
import { validateSettings } from "../lib/types";

const phaseText: Record<SpeedPhase, string> = {
  idle: "",
  tcping: "正在采样 Cloudflare 网段，TCP 连通性测试中…",
  health: "正在对候选 IP 进行 Worker 健康检查…",
  done: "测速完成",
  error: "测速失败",
};

/** 超过该时长未收到任何事件即判定为疑似卡死 */
const STALL_THRESHOLD_MS = 45_000;

const clampInt = (v: number, min: number, max: number) =>
  Number.isFinite(v) ? Math.min(max, Math.max(min, Math.trunc(v))) : min;

export function SpeedTest() {
  const settings = useSettings((s) => s.settings);
  const { phase, opts, rows, progress, bestIp, running, setOpts, run, stop } = useSpeed();
  const navigate = useUi((s) => s.navigate);
  const toast = useUi((s) => s.toast);
  const applyIp = useSettings((s) => s.applyIp);
  const [advanced, setAdvanced] = useState(false);
  const [popKey, setPopKey] = useState<number | null>(null);
  const [stalled, setStalled] = useState(false);

  const pct01 = (opts.total - 1000) / 19000;
  const labelLeft = `max(10px, min(calc(10px + (100% - 20px) * ${pct01}), calc(100% - 10px)))`;

  // 疑似卡死 watchdog：运行中且长时间无事件则提示
  useEffect(() => {
    if (!running) {
      setStalled(false);
      return;
    }
    const id = setInterval(() => {
      const last = useSpeed.getState().lastEventAt;
      if (Date.now() - last > STALL_THRESHOLD_MS) setStalled(true);
    }, 5000);
    return () => clearInterval(id);
  }, [running]);

  const start = async () => {
    if (running) return;
    const errs = validateSettings(settings);
    if (Object.keys(errs).length) {
      toast("Worker 配置不完整，请先到「代理」页完善", "error");
      navigate("proxy");
      return;
    }
    setStalled(false);
    await run(settings);
  };

  const onStop = async () => {
    setStalled(false);
    await stop();
  };

  const apply = async () => {
    if (!bestIp) return;
    await applyIp(bestIp);
  };

  const progressPct =
    progress && progress.total > 0 ? (progress.tested / progress.total) * 100 : 0;

  return (
    <div className="flex flex-col gap-6">
      <Panel title="测速参数">
        <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
          <span className="w-24 shrink-0 text-[13.5px] text-ink3">采样数量</span>
          <div className="flex-1">
            <div className="relative pt-6">
              <span
                key={popKey ?? "init"}
                className={clsx(
                  "tnum pointer-events-none absolute top-[3px] z-10 -translate-x-1/2 rounded-pill bg-surface px-2 py-0.5 font-mono text-[11.5px] font-semibold text-accent shadow-card",
                  popKey !== null && "value-pop",
                )}
                style={{ left: labelLeft }}
              >
                {opts.total.toLocaleString()}
              </span>
              <input
                type="range"
                min={1000}
                max={20000}
                step={500}
                disabled={running}
                value={opts.total}
                onChange={(e) => {
                  const v = Number(e.target.value);
                  setOpts({ total: v });
                  setPopKey(v);
                }}
                className="fp-range h-1.5 w-full cursor-pointer appearance-none rounded-full bg-track disabled:opacity-50"
                aria-label="采样数量"
              />
            </div>
            <div className="mt-1.5 flex justify-between text-[11.5px] text-ink3">
              <span>1,000</span>
              <span>20,000</span>
            </div>
          </div>
        </ListRow>
        <ListRow>
          <span className="w-24 shrink-0 text-[13.5px] text-ink3">连接方式</span>
          <span className="flex flex-1 items-center justify-between gap-3">
            <span className="text-[12.5px] text-ink3">
              HTTP :80 直连探测（绕过 DNS 污染与 SNI 阻断）
            </span>
          </span>
        </ListRow>
        <button
          type="button"
          onClick={() => setAdvanced((v) => !v)}
          className="press flex w-full items-center justify-between px-[clamp(17px,3vw,24px)] py-3 text-[12.5px] font-medium text-ink3 hover:text-ink"
        >
          高级参数
          {advanced ? <ChevronUp size={15} /> : <ChevronDown size={15} />}
        </button>
        {advanced && (
          <div className="divide-y divide-hairline border-t border-hairline">
            <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
              <span className="w-24 shrink-0 text-[13.5px] text-ink3">并发</span>
              <Input
                mono
                label="TCP 并发数（1–512）"
                inputMode="numeric"
                disabled={running}
                value={String(opts.tcpingLimit)}
                onChange={(e) =>
                  setOpts({ tcpingLimit: clampInt(Number(e.target.value), 1, 512) })
                }
              />
            </ListRow>
            <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
              <span className="w-24 shrink-0 text-[13.5px] text-ink3">TCP 超时</span>
              <Input
                mono
                label="TCP 超时（ms）"
                inputMode="numeric"
                disabled={running}
                value={String(opts.tcpingTimeoutMs)}
                onChange={(e) =>
                  setOpts({ tcpingTimeoutMs: clampInt(Number(e.target.value), 1, 30_000) })
                }
              />
            </ListRow>
            <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
              <span className="w-24 shrink-0 text-[13.5px] text-ink3">健康检查</span>
              <Input
                mono
                label="候选数量（4–64）"
                inputMode="numeric"
                disabled={running}
                value={String(opts.healthLimit)}
                onChange={(e) =>
                  setOpts({ healthLimit: clampInt(Number(e.target.value), 4, 64) })
                }
              />
            </ListRow>
            <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
              <span className="w-24 shrink-0 text-[13.5px] text-ink3">检查超时</span>
              <Input
                mono
                label="健康检查超时（ms）"
                inputMode="numeric"
                disabled={running}
                value={String(opts.healthTimeoutMs)}
                onChange={(e) =>
                  setOpts({ healthTimeoutMs: clampInt(Number(e.target.value), 100, 60_000) })
                }
              />
            </ListRow>
          </div>
        )}
      </Panel>

      <div className="flex flex-col items-stretch gap-2">
        <Button
          variant={running ? "danger" : "primary"}
          onClick={running ? onStop : start}
        >
          {running ? (
            <>
              <Square size={14} fill="currentColor" />
              停止测速
            </>
          ) : (
            <>
              <Play size={16} />
              {phase === "done" ? "重新测速" : "开始测速"}
            </>
          )}
        </Button>
        {phase === "done" && bestIp &&
          (settings.prefIp === bestIp ? (
            <span className="flex items-center justify-center gap-1.5 rounded-pill border border-hairline bg-surface px-4 py-2.5 text-[13px] text-ink2 shadow-card">
              <Badge variant="live">已应用</Badge>
              <span className="tnum truncate font-mono text-[12.5px] text-ink3">
                {bestIp}
              </span>
            </span>
          ) : (
            <Button variant="secondary" onClick={apply}>
              应用最佳 IP：{bestIp}
            </Button>
          ))}
      </div>

      {phase !== "idle" && (
        <Panel>
          <ListRow>
            <span className="flex-1 text-[13.5px] text-ink2">{phaseText[phase]}</span>
            {progress && (
              <span className="tnum font-mono text-[12px] text-ink3">
                {progress.tested.toLocaleString()} / {progress.total.toLocaleString()}
              </span>
            )}
          </ListRow>
          {(phase === "tcping" || phase === "health") && (
            <div className="px-[clamp(17px,3vw,24px)] pb-4">
              {progress ? <Progress value={progressPct} /> : <Progress indeterminate />}
            </div>
          )}
          {stalled && (
            <p className="px-[clamp(17px,3vw,24px)] pb-4 text-[12.5px] text-heat">
              长时间未收到进度更新（可能网络异常），可点击「停止测速」中止本次测试。
            </p>
          )}
          {phase === "error" && (
            <p className="px-[clamp(17px,3vw,24px)] pb-4 text-[13px] leading-relaxed text-error">
              请检查 Worker 配置、网络连通性后重试。
            </p>
          )}
        </Panel>
      )}

      {rows.length > 0 && (
        <Panel title={phase === "done" ? `结果 · ${rows.length} 条 · 点击行应用` : "结果"}>
          <div className="divide-y divide-hairline">
            {rows.map((r, i) => {
              const isApplied = settings.prefIp === r.ip;
              return (
                <button
                  key={r.ip}
                  type="button"
                  title="点击应用此 IP"
                  onClick={() => void applyIp(r.ip)}
                  className={clsx(
                    "press flex w-full items-center gap-3 px-[clamp(17px,3vw,24px)] py-2.5 text-left",
                    isApplied ? "bg-accent/5" : "hover:bg-hover",
                  )}
                >
                  <span className="tnum w-5 shrink-0 text-right font-mono text-[11.5px] text-faint">
                    {i + 1}
                  </span>
                  <span
                    className={clsx(
                      "tnum flex-1 truncate font-mono text-[13.5px]",
                      isApplied ? "text-accent-link" : "text-ink",
                    )}
                  >
                    {r.ip}
                  </span>
                  {isApplied && <Badge variant="live">应用</Badge>}
                  {i < 3 && <Badge variant="heat">TOP {i + 1}</Badge>}
                  <span className="tnum font-mono text-[13px] text-ink2">
                    {r.rttMs.toFixed(1)} ms
                  </span>
                </button>
              );
            })}
          </div>
        </Panel>
      )}
    </div>
  );
}
