import clsx from "clsx";
import { useState } from "react";
import {
  AEADS,
  COMPRESSORS,
  validateSettings,
  type SettingsErrors,
} from "../lib/types";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Input } from "../components/ui/Input";
import { ListRow, Panel } from "../components/ui/Panel";
import { Segmented } from "../components/ui/Segmented";
import { Select } from "../components/ui/Select";
import { useProxy } from "../store/proxy";
import { useSettings } from "../store/settings";
import { useUi } from "../store/ui";

type ErrorKey = keyof SettingsErrors;

export function ProxySettingsPage() {
  const { settings, saved, loading, patch, save } = useSettings();
  const { status, health, healthBusy, checkHealth } = useProxy();
  const toast = useUi((s) => s.toast);
  const [touched, setTouched] = useState<Partial<Record<ErrorKey, boolean>>>({});
  const [saving, setSaving] = useState(false);

  const dirty = saved !== null && JSON.stringify(settings) !== JSON.stringify(saved);
  const restartNeeded =
    status.running && dirty && ["domain", "useHttps", "localPort"].some(
      (k) => JSON.stringify(settings[k as keyof typeof settings]) !== JSON.stringify(saved?.[k as keyof typeof saved]),
    );

  // 校验错误由 settings 派生（单一事实来源），touched 决定是否展示：
  // 输入过的字段随输入即时更新错误，未触碰的字段等提交时再报，避免一打开就满屏红色。
  const errs = validateSettings(settings);
  const field = (k: ErrorKey) => (touched[k] ? errs[k] : undefined);
  const markTouched = (k: ErrorKey) =>
    setTouched((t) => (t[k] ? t : { ...t, [k]: true }));
  const touchAll = () =>
    setTouched({ domain: true, localPort: true, authKey: true, prefIp: true });

  const sectionBadge = (keys: ErrorKey[]) =>
    keys.some((k) => errs[k]) ? <Badge variant="error">未完成</Badge> : undefined;

  const firstError = (e: SettingsErrors): ErrorKey | undefined =>
    Object.keys(e)[0] as ErrorKey | undefined;

  const onSave = async () => {
    const e = validateSettings(settings);
    touchAll();
    const firstErr = firstError(e);
    if (firstErr) {
      toast(`配置有误：${e[firstErr]}`, "error");
      return;
    }
    setSaving(true);
    try {
      await save();
      toast("设置已保存", "success");
    } catch (err) {
      toast(`保存失败：${err}`, "error");
    } finally {
      setSaving(false);
    }
  };

  const onVerify = () => {
    const e = validateSettings(settings);
    touchAll();
    const firstErr = firstError(e);
    if (firstErr) {
      toast(`配置有误：${e[firstErr]}`, "error");
      return;
    }
    void checkHealth(settings);
  };

  if (loading) {
    return (
      <div className="flex flex-col items-center gap-3 py-24 text-ink3">
        <div className="h-5 w-5 animate-spin rounded-full border-2 border-hairline border-t-accent" />
        <span className="text-[13px]">正在加载设置…</span>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <Panel title="Worker 连接" right={sectionBadge(["domain", "authKey"])}>
        <div className="divide-y divide-hairline">
          <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">域名</span>
            <Input
              mono
              label="Worker 域名"
              placeholder="free-proxy.xxx.workers.dev"
              value={settings.domain}
              onChange={(e) => patch({ domain: e.target.value })}
              onBlur={() => markTouched("domain")}
              error={field("domain")}
              // hint="如free-proxy.xxxxxx.workers.dev"
            />
          </ListRow>
          <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">协议</span>
            <div className="flex flex-1 flex-wrap items-center justify-between gap-3">
              <Segmented
                options={[
                  { value: "on", label: "HTTPS" },
                  { value: "off", label: "HTTP" },
                ]}
                value={settings.useHttps ? "on" : "off"}
                onChange={(v) => patch({ useHttps: v === "on" })}
              />
            </div>
          </ListRow>
          <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">密钥</span>
            <Input
              label="认证密钥"
              type="password"
              placeholder="与 Worker 部署时配置的 key 一致"
              value={settings.authKey}
              onChange={(e) => patch({ authKey: e.target.value })}
              onBlur={() => markTouched("authKey")}
              error={field("authKey")}
            />
          </ListRow>
        </div>
      </Panel>

      <Panel title="本地代理" right={sectionBadge(["localPort", "prefIp"])}>
        <div className="divide-y divide-hairline">
          <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">端口</span>
            <Input
              mono
              label="本地监听端口"
              inputMode="numeric"
              value={String(settings.localPort)}
              onChange={(e) => patch({ localPort: Number(e.target.value) || 0 })}
              onBlur={() => markTouched("localPort")}
              error={field("localPort")}
            />
          </ListRow>
          <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">优选 IP</span>
            <Input
              mono
              label="优选 IP（可选）"
              placeholder="留空走 DNS 解析，可在测速页自动选择"
              value={settings.prefIp ?? ""}
              onChange={(e) => patch({ prefIp: e.target.value || null })}
              onBlur={() => markTouched("prefIp")}
              error={field("prefIp")}
            />
          </ListRow>
        </div>
      </Panel>

      <Panel title="算法组合">
        <div className="divide-y divide-hairline">
          <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">加密</span>
            <Select
              aria-label="AEAD 加密算法"
              value={settings.aead}
              onChange={(e) => patch({ aead: e.target.value as typeof settings.aead })}
            >
              {AEADS.map((a) => (
                <option key={a.value} value={a.value}>
                  {a.label}
                </option>
              ))}
            </Select>
          </ListRow>
          <ListRow className="flex-col items-stretch gap-2 sm:flex-row sm:items-center sm:gap-4">
            <span className="w-20 shrink-0 text-[13.5px] text-ink3">压缩</span>
            <Select
              aria-label="压缩算法"
              value={settings.compressor}
              onChange={(e) => patch({ compressor: e.target.value as typeof settings.compressor })}
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

      <div className="flex flex-wrap items-center gap-3">
        <Button loading={saving} onClick={onSave}>
          保存设置
        </Button>
        <Button
          variant="secondary"
          loading={healthBusy}
          onClick={onVerify}
        >
          验证 Worker
        </Button>
        {health !== null && (
          <Badge variant={health ? "live" : "error"}>
            {health ? "连接正常" : "连接异常"}
          </Badge>
        )}
        <div className="flex flex-wrap items-center gap-2">
          {dirty && (
            <span className="flex items-center gap-1.5 text-[12px] text-ink3">
              <span className="h-1.5 w-1.5 rounded-full bg-heat" />
              有未保存的修改
            </span>
          )}
          {restartNeeded && (
            <span
              className={clsx(
                "inline-flex items-center rounded-pill bg-accent/10 px-2.5 py-1 text-[11.5px] font-medium text-accent-link",
              )}
            >
              连接类改动需重启代理
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
