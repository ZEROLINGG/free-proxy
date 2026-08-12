import { Copy, FolderOpen, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, appVersion } from "../lib/tauri";
import { Button } from "../components/ui/Button";
import { ListRow, Panel } from "../components/ui/Panel";
import { useProxy } from "../store/proxy";
import { useUi } from "../store/ui";

export function CaCert() {
  const [ca, setCa] = useState<{ path: string; certPem: string } | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [copying, setCopying] = useState(false);
  const { status } = useProxy();
  const toast = useUi((s) => s.toast);

  useEffect(() => {
    api
      .caInfo()
      .then((info) => setCa(info))
      .catch((e) => toast(`读取 CA 证书失败：${e}`, "error"))
      .finally(() => setLoading(false));
    appVersion().then(setVersion);
  }, [toast]);

  const copy = async () => {
    if (!ca) return;
    setCopying(true);
    try {
      await writeText(ca.certPem);
      toast("证书已复制到剪贴板", "success");
    } catch (e) {
      toast(`复制失败：${e}`, "error");
    } finally {
      setCopying(false);
    }
  };

  const openDir = async () => {
    try {
      await api.openCaDir();
    } catch (e) {
      toast(String(e), "error");
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <Panel title="根证书">
        {loading ? (
          <div className="flex flex-col items-center gap-3 py-16 text-ink3">
            <div className="h-5 w-5 animate-spin rounded-full border-2 border-hairline border-t-accent" />
            <span className="text-[13px]">正在加载…</span>
          </div>
        ) : ca ? (
          <>
            <ListRow>
              <span className="w-20 shrink-0 text-[13.5px] text-ink3">路径</span>
              <span
                className="tnum min-w-0 flex-1 truncate font-mono text-[12.5px] text-ink"
                title={ca.path}
              >
                {ca.path}
              </span>
            </ListRow>
            <ListRow className="items-start">
              <span className="w-20 shrink-0 text-[13.5px] text-ink3">内容</span>
              <pre className="max-h-56 min-w-0 flex-1 overflow-auto rounded-thumb bg-ground p-3 font-mono text-[11px] leading-relaxed text-ink2">
                {ca.certPem}
              </pre>
            </ListRow>
            <div className="flex flex-wrap gap-2.5 px-[clamp(17px,3vw,24px)] pb-4 pt-1">
              <Button variant="secondary" className="h-9 px-4 text-[13px]" loading={copying} onClick={copy}>
                <Copy size={15} />
                复制 PEM
              </Button>
              <Button variant="secondary" className="h-9 px-4 text-[13px]" onClick={openDir}>
                <FolderOpen size={15} />
                打开证书目录
              </Button>
            </div>
          </>
        ) : (
          <p className="px-[clamp(17px,3vw,24px)] py-6 text-[13px] text-error">
            读取证书失败
          </p>
        )}
      </Panel>

      <Panel title="信任步骤">
        <div className="divide-y divide-hairline">
          <ListRow className="items-start">
            <span className="tnum shrink-0 font-mono text-[12.5px] font-semibold text-accent">
              01
            </span>
            <p className="text-[13.5px] leading-relaxed text-ink">
              安装根证书：点击上方「打开证书目录」，双击{" "}
              <code className="rounded bg-ground px-1 py-0.5 font-mono text-[12px] text-ink2">
                ca.crt
              </code>{" "}
              并加入系统信任（macOS：钥匙串 → 始终信任；Windows：证书导入 → 受信任的根证书颁发机构）。
            </p>
          </ListRow>
          <ListRow className="items-start">
            <span className="tnum shrink-0 font-mono text-[12.5px] font-semibold text-accent">
              02
            </span>
            <p className="text-[13.5px] leading-relaxed text-ink">
              在系统或浏览器中配置 HTTP 代理，指向本地地址{" "}
              <code className="rounded bg-ground px-1 py-0.5 font-mono text-[12px] text-ink2">
                {status.running ? `127.0.0.1:${status.port}` : "127.0.0.1:<代理端口>"}
              </code>
              。
            </p>
          </ListRow>
          <ListRow className="items-start">
            <span className="tnum shrink-0 font-mono text-[12.5px] font-semibold text-accent">
              03
            </span>
            <p className="text-[13.5px] leading-relaxed text-ink">
              返回仪表盘启动代理，访问任意 HTTPS 站点验证解密链路。
            </p>
          </ListRow>
        </div>
      </Panel>

      <div className="flex items-center justify-center gap-2 pt-1 text-[12px] text-ink3">
        <ShieldCheck size={14} />
        <span>
          MITM 证书仅在本地生成，密钥不出本机
          {version ? ` · v${version}` : ""}
        </span>
      </div>
    </div>
  );
}
