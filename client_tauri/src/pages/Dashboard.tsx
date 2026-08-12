import { Activity, ChevronRight, ShieldCheck } from "lucide-react";
import { useState } from "react";
import {
  AEADS,
  COMPRESSORS,
  isAead,
  isCompressor,
  validateSettings,
  type Aead,
  type Compressor,
} from "../lib/types";
import { Badge, StatusDot } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { ColoredCta } from "../components/ui/ColoredCta";
import { ListRow, Panel } from "../components/ui/Panel";
import { Select } from "../components/ui/Select";
import { useProxy } from "../store/proxy";
import { useSettings } from "../store/settings";
import { useUi } from "../store/ui";

export function Dashboard() {
  const settings = useSettings((s) => s.settings);
  const {
    status,
    busy,
    health,
    healthBusy,
    availability,
    availabilityBusy,
    start,
    stop,
    checkHealth,
    checkAvailability,
    setAead,
    setCompressor,
  } = useProxy();
  const navigate = useUi((s) => s.navigate);
  const toast = useUi((s) => s.toast);
  const [hotBusy, setHotBusy] = useState(false);

  // 运行中显示实际生效值；未运行时回退到设置值（下次启动生效）
  const shownAead: Aead = status.running && isAead(status.aead) ? status.aead : settings.aead;
  const shownComp: Compressor =
    status.running && isCompressor(status.compressor) ? status.compressor : settings.compressor;

  const toggle = async () => {
    if (busy) return;
    if (status.running) {
      try {
        await stop();
      } catch (e) {
        toast(String(e), "error");
      }
      return;
    }
    const errs = validateSettings(settings);
    if (Object.keys(errs).length) {
      toast("代理配置不完整，请先到「代理」页完善", "error");
      navigate("proxy");
      return;
    }
    try {
      await start(settings);
    } catch (e) {
      toast(String(e), "error");
    }
  };

  const onHotAead = async (v: Aead) => {
    if (hotBusy) return;
    if (!status.running) {
      useSettings.getState().patch({ aead: v });
      toast("已更新设置，下次启动生效", "info");
      return;
    }
    setHotBusy(true);
    try {
      await setAead(v);
      useSettings.getState().patch({ aead: v });
      toast("加密算法已生效，无需重启", "success");
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setHotBusy(false);
    }
  };

  const onHotComp = async (v: Compressor) => {
    if (hotBusy) return;
    if (!status.running) {
      useSettings.getState().patch({ compressor: v });
      toast("已更新设置，下次启动生效", "info");
      return;
    }
    setHotBusy(true);
    try {
      await setCompressor(v);
      useSettings.getState().patch({ compressor: v });
      toast("压缩算法已生效，无需重启", "success");
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setHotBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-8">
      <ColoredCta running={status.running} busy={busy} onClick={toggle} />

      <Panel title="运行状态">
        <div className="divide-y divide-hairline">
          <ListRow>
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">状态</span>
            <StatusDot on={status.running} />
          </ListRow>
          <ListRow>
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">监听端口</span>
            <span className="tnum font-mono text-[13.5px] text-ink">
              {status.running ? `${status.port}` : "—"}
            </span>
          </ListRow>
          <ListRow>
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">优选 IP</span>
            <span className="tnum truncate font-mono text-[13.5px] text-ink">
              {status.running
                ? (status.ip ?? "DNS 自动解析")
                : (settings.prefIp
                    ? `${settings.prefIp}（下次启动生效）`
                    : "DNS 自动解析")}
            </span>
          </ListRow>
          <ListRow>
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">算法</span>
            <span className="flex flex-wrap items-center gap-1.5">
              <Badge>{shownComp}</Badge>
              <Badge variant="accent">{shownAead}</Badge>
            </span>
          </ListRow>
          <ListRow>
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">链路</span>
            <span className="flex min-w-0 flex-1 items-center justify-between gap-3">
              {!status.running ? (
                <span className="text-[13px] text-ink3">—</span>
              ) : availabilityBusy ? (
                <span className="text-[13px] text-ink3">正在检测…</span>
              ) : availability === null ? (
                <span className="text-[13px] text-ink3">未检测</span>
              ) : availability.ok ? (
                <span className="flex min-w-0 items-center gap-2">
                  <Badge variant="live">链路正常</Badge>
                  <span className="tnum truncate font-mono text-[12px] text-ink3">
                    出口 IP {availability.ip}
                    {availability.latencyMs != null && `（${availability.latencyMs} ms）`}
                  </span>
                </span>
              ) : (
                <span className="flex min-w-0 items-center gap-2">
                  <Badge variant="error">链路异常</Badge>
                  <span
                    className="truncate text-[12px] text-ink3"
                    title={availability.error ?? ""}
                  >
                    {availability.error}
                  </span>
                </span>
              )}
              {status.running && (
                <Button
                  variant="secondary"
                  className="h-8 shrink-0 px-3.5 text-[12.5px]"
                  loading={availabilityBusy}
                  onClick={() => void checkAvailability()}
                >
                  重测
                </Button>
              )}
            </span>
          </ListRow>
          <ListRow>
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">Worker</span>
            <span className="flex flex-1 items-center justify-between gap-3">
              {health === null ? (
                <span className="text-[13px] text-ink3">未检测</span>
              ) : health ? (
                <Badge variant="live">连接正常</Badge>
              ) : (
                <Badge variant="error">连接异常</Badge>
              )}
              <Button
                variant="secondary"
                className="h-8 px-3.5 text-[12.5px]"
                loading={healthBusy}
                onClick={() => checkHealth(settings)}
              >
                检测
              </Button>
            </span>
          </ListRow>
        </div>
      </Panel>

      <Panel title={status.running ? "热切换算法" : "算法（启动时生效）"}>
        {!status.running && (
          <p className="px-[clamp(17px,3vw,24px)] pt-3 text-[12.5px] text-ink3">
            代理未运行，算法修改将在下次启动时生效。
          </p>
        )}
        <div className="divide-y divide-hairline">
          <ListRow>
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">加密</span>
            <Select
              aria-label="加密算法"
              disabled={hotBusy}
              value={shownAead}
              onChange={(e) => void onHotAead(e.target.value as Aead)}
            >
              {AEADS.map((a) => (
                <option key={a.value} value={a.value}>
                  {a.label}
                </option>
              ))}
            </Select>
          </ListRow>
          <ListRow>
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">压缩</span>
            <Select
              aria-label="压缩算法"
              disabled={hotBusy}
              value={shownComp}
              onChange={(e) => void onHotComp(e.target.value as Compressor)}
            >
              {COMPRESSORS.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </Select>
          </ListRow>
        </div>
      </Panel>

      <Panel>
        <ListRow onClick={() => navigate("speed")} className="group">
          <Activity size={20} strokeWidth={1.75} className="text-ink3 group-hover:text-accent" />
          <span className="flex-1 text-[15px] font-medium text-ink">IP 优选测速</span>
          <ChevronRight size={16} className="text-faint" />
        </ListRow>
        <ListRow onClick={() => navigate("ca")} className="group">
          <ShieldCheck size={20} strokeWidth={1.75} className="text-ink3 group-hover:text-accent" />
          <span className="flex-1 text-[15px] font-medium text-ink">CA 证书安装</span>
          <ChevronRight size={16} className="text-faint" />
        </ListRow>
      </Panel>
    </div>
  );
}
