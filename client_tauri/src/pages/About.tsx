import { useEffect, useState } from "react";
import { appVersion } from "../lib/tauri";
import { AEADS, COMPRESSORS } from "../lib/types";
import { ListRow, Panel } from "../components/ui/Panel";
import { Segmented } from "../components/ui/Segmented";
import { useSettings } from "../store/settings";
import { useUi } from "../store/ui";
import { PlaneTakeoff } from 'lucide-react';


export function About() {
  const [version, setVersion] = useState<string | null>(null);
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);
  const current = useSettings((s) => s.settings);

  useEffect(() => {
    appVersion().then(setVersion);
  }, []);

  return (
    <div className="flex flex-col gap-6">
      <Panel>
        <div className="flex flex-col items-center gap-2 px-6 py-10 text-center">
          <div className="flex h-16 w-16 items-center justify-center rounded-hero bg-[linear-gradient(135deg,#2EFFFF,#5CFFAD)] dark:bg-[linear-gradient(135deg,#00E3E3,#00E372)] shadow-cta">
            <PlaneTakeoff size={32} />
          </div>
          <h2 className="mt-2 text-[21px] font-bold tracking-[-0.02em] text-ink">
            free-proxy
          </h2>
          <p className="text-[13px] text-ink3">
            基于 Cloudflare Worker 的加密流量代理 · {version ? `v${version}` : ""}
          </p>
        </div>
      </Panel>

      <Panel title="外观">
        <ListRow>
          <span className="flex-1 text-[14px] text-ink">主题</span>
          <Segmented
              options={[
                {value: "auto", label: "自动"},
                {value: "light", label: "浅色" },
              { value: "dark", label: "深色" },
            ]}
            value={theme}
            onChange={(v) => setTheme(v)}
          />
        </ListRow>
      </Panel>

      <Panel title="协议组合矩阵">
        <div className="overflow-x-auto px-[clamp(17px,3vw,24px)] py-4">
          <table className="w-full border-collapse">
            <thead>
              <tr>
                <th className="pb-2 text-left text-[11.5px] font-medium text-ink3">
                  压缩 \ 加密
                </th>
                {AEADS.map((a) => (
                  <th
                    key={a.value}
                    className="pb-2 text-right text-[10.5px] font-medium text-ink3"
                  >
                    {a.label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {COMPRESSORS.map((c) => (
                <tr key={c.value}>
                  <td className="py-1.5 pr-3 text-[12.5px] font-medium text-ink2">
                    {c.label}
                  </td>
                  {AEADS.map((a) => {
                    const isCurrent =
                      current.compressor === c.value && current.aead === a.value;
                    return (
                      <td key={a.value} className="py-1.5 text-center">
                        <span
                          className={
                            isCurrent
                              ? "inline-flex h-6 w-6 items-center justify-center rounded-full bg-accent text-[10.5px] font-bold text-white"
                              : "text-[11px] text-ink4"
                          }
                        >
                          {isCurrent ? "✓" : "·"}
                        </span>
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Panel>

      <Panel>
        <ListRow>
          <span className="text-[14px] text-ink">技术栈</span>
          <span className="ml-auto text-[12.5px] text-ink3">
            Tauri 2 · React 19 · Rust · Cloudflare Workers
          </span>
        </ListRow>
      </Panel>
    </div>
  );
}
