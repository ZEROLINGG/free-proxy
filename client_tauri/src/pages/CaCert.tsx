import clsx from "clsx";
import { ChevronDown, Copy, FolderOpen, ShieldCheck, Terminal } from "lucide-react";
import { platform, type Platform } from "@tauri-apps/plugin-os";
import { useEffect, useState, type ReactNode } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, appVersion } from "../lib/tauri";
import type { CaInfo } from "../lib/types";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { ListRow, Panel } from "../components/ui/Panel";
import { useUi } from "../store/ui";

const PLATFORM_LABEL: Record<string, string> = {
  linux: "Linux",
  macos: "macOS",
  windows: "Windows",
  android: "Android",
  ios: "iOS",
};

/** 一键安装的授权提示文案（按平台） */
const AUTH_HINT: Partial<Record<Platform, string>> = {
  linux: "将弹出 pkexec 授权窗口",
  macos: "将请求管理员密码",
  windows: "将弹出 UAC 用户账户控制",
};

function Step({
  n,
  children,
  highlight,
}: {
  n: string;
  children: ReactNode;
  highlight?: boolean;
}) {
  return (
    <ListRow className="items-start">
      <span
        className={clsx(
          "tnum shrink-0 font-mono text-[12.5px] font-semibold",
          highlight ? "text-error" : "text-accent",
        )}
      >
        {n}
      </span>
      <p className={clsx("text-[13.5px] leading-relaxed", highlight ? "text-error" : "text-ink")}>
        {children}
      </p>
    </ListRow>
  );
}

function Code({ children }: { children: string }) {
  return (
    <code className="rounded bg-ground px-1 py-0.5 font-mono text-[12px] text-ink2">
      {children}
    </code>
  );
}

/** 桌面端：按平台渲染的一键安装说明 */
function DesktopInstall({ platform }: { platform: Platform }) {
    switch (platform) {
        case "linux":
            return (
                <>
                    <Step n="01">
                        <strong>安装系统证书：</strong>
                        <br />
                        点击上方「打开证书目录」，在目录中打开终端，执行以下命令导入系统信任库（以 Ubuntu 为例）：
                        <br />
                        <Code>
                            sudo cp ca.crt.pem /usr/local/share/ca-certificates/ca.crt &amp;&amp; sudo update-ca-certificates
                        </Code>
                    </Step>
                    <Step n="02">
                        <strong>Linux 浏览器单独导入：</strong>
                        <br />
                        由于 Linux 浏览器不信任系统证书，需在浏览器内手动导入：
                        <br />
                        Chrome/Edge：地址栏输入 <Code>chrome://settings/certificates</Code> → 切换到 <strong>受信任的根证书颁发机构</strong> → 导入。
                        <br />
                        Firefox：设置 → 隐私与安全 → 证书 → 查看证书 → <strong>证书颁发机构</strong> → 导入。
                    </Step>
                </>
            );
        case "macos":
            return (
                <>
                    <Step n="01">
                        点击上方「打开证书目录」，双击文件 <Code>ca.crt.pem</Code>，系统将自动打开「钥匙串访问」。
                    </Step>
                    <Step n="02">
                        在钥匙串中找到名为 <strong>com.zz.freeproxy</strong>（或您的证书名称）的证书，双击它打开详情窗口。
                    </Step>
                    <Step n="03">
                        展开 <strong>信任</strong> 菜单，将「使用此证书时」的选项改为 <strong className="text-blue-600">始终信任</strong>。
                    </Step>
                    <Step n="04">
                        <strong className="text-red-500">关键步骤：</strong>直接关闭当前详情窗口，系统会要求输入 Mac 开机密码或触控 ID 进行授权，完成即可。
                    </Step>
                </>
            );
        case "windows":
            return (
                <>
                    <Step n="01">
                        点击上方「打开证书目录」，双击打开文件 <Code>ca.crt.pem</Code>，在弹出的窗口中点击 <strong>安装证书...</strong>。
                    </Step>
                    <Step n="02">
                        在导入向导中，存储位置选择 <strong>当前用户</strong>，点击下一页。
                    </Step>
                    <Step n="03">
                        选择 <strong>将所有的证书都放入下列存储</strong>，点击右侧的 <strong>浏览...</strong>。
                    </Step>
                    <Step n="04">
                        在弹出的目录中选中 <strong className="text-blue-600">受信任的根证书颁发机构</strong>，点击确定。
                    </Step>
                    <Step n="05">
                        一路点击「下一页」直到「完成」。若弹出安全警告（“你正准备安装一个证书...”），点击 <strong>是(Y)</strong> 即可。
                    </Step>
                </>
            );
        default:
            return null;
    }
}

/** 移动端：Android / iOS 专属安装指引 */
function MobileGuide({ platform }: { platform: Platform;}) {
  if (platform === "android") {
    return (
      <>
        <Step n="01">
          复制本页的 PEM 证书内容，保存为扩展名为 <Code>.crt</Code> 的文件（如{" "}
          <Code>ca.crt</Code>）。
        </Step>
        <Step n="02">
          系统设置 → 安全 → 加密与凭据 → 安装证书 → CA 证书 → 选择文件 → 输入锁屏 PIN 确认安装。
        </Step>
      </>
    );
  }
  return (
    <>
      <Step n="01">
        复制本页的 PEM 证书内容，保存为 <Code>.crt</Code> 文件（如 <Code>ca.crt</Code>）。
      </Step>
      <Step n="02">
        设置 → 通用 → VPN 与设备管理 → 找到证书描述文件 → 点击「安装」（需锁屏密码）。
      </Step>
      <Step n="03" highlight>
        关键：设置 → 通用 → 关于本机 → 证书信任设置 → 开启该证书的「完全信任」。
        不开启则 HTTPS 解密握手会失败。
      </Step>
    </>
  );
}

/** 检测运行平台；非 Tauri 环境（浏览器预览）静默回退 null */
function detectPlatform(): Platform | null {
  try {
    return platform();
  } catch {
    return null;
  }
}

export function CaCert() {
  const [ca, setCa] = useState<CaInfo | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [copying, setCopying] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const [showManual, setShowManual] = useState(false);
  const platformName = detectPlatform();
  const toast = useUi((s) => s.toast);

  const isMobile = platformName === "android" || platformName === "ios";
  const isDesktop = !!platformName && !isMobile;

  useEffect(() => {
    api
      .caInfo()
      .then((info) => {
        setCa(info);
        if (info.rebuilt) {
          toast("CA 已自动重建（设备标识变化），请重新导入根证书", "info");
        }
      })
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

  const install = async () => {
    if (!ca) return;
    setInstalling(true);
    setInstallError(null);
    try {
      await api.installCa();
      toast("已安装到系统信任库", "success");
    } catch (e) {
      setInstallError(String(e));
      setShowManual(true);
      toast(`自动安装失败：${e}`, "error");
    } finally {
      setInstalling(false);
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
            {ca.rebuilt && (
              <div className="mx-[clamp(17px,3vw,24px)] mt-3 flex items-start gap-2 rounded-thumb bg-error-bg px-3.5 py-2.5 text-[12.5px] leading-relaxed text-error">
                <ShieldCheck size={15} className="mt-0.5 shrink-0" />
                <span>
                  CA 已自动重建（设备标识或密钥文件变化），旧证书已失效，请按下方步骤重新安装根证书。
                </span>
              </div>
            )}
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
              {!isMobile && (
                <Button variant="secondary" className="h-9 px-4 text-[13px]" onClick={openDir}>
                  <FolderOpen size={15} />
                  打开证书目录
                </Button>
              )}
            </div>
          </>
        ) : (
          <p className="px-[clamp(17px,3vw,24px)] py-6 text-[13px] text-error">
            读取证书失败
          </p>
        )}
      </Panel>

      <Panel title="安装根证书">
        <div className="divide-y divide-hairline">
          {platformName ? (
            <ListRow className="items-start">
              <span className="w-20 shrink-0 text-[13.5px] text-ink3">平台</span>
              <div className="flex flex-1 flex-wrap items-center gap-2.5">
                <Badge variant="neutral">{PLATFORM_LABEL[platformName]}</Badge>
                <span className="text-[12.5px] text-ink3">
                  {isMobile
                    ? "移动端无法静默安装系统证书，请按下方步骤手动导入"
                    : `一键安装会以系统权限写入信任库${AUTH_HINT[platformName] ? `（${AUTH_HINT[platformName]}）` : ""}`}
                </span>
              </div>
            </ListRow>
          ) : (
            <p className="px-[clamp(17px,3vw,24px)] py-3.5 text-[13px] text-ink2">
              无法识别运行平台，以下为通用安装指引。
            </p>
          )}

          {isDesktop && platformName && (
            <div className="px-[clamp(17px,3vw,24px)] pb-4 pt-3.5">
              <Button
                variant="primary"
                className="w-full"
                loading={installing}
                disabled={!ca}
                onClick={install}
              >
                <ShieldCheck size={16} />
                一键安装到系统信任库
              </Button>
            </div>
          )}

          {isDesktop && platformName && (
            <div>
              <button
                type="button"
                onClick={() => setShowManual((s) => !s)}
                className="row-hover flex w-full items-center justify-between px-[clamp(17px,3vw,24px)] py-3.5 text-left text-[13px] font-medium text-ink2"
              >
                <span className="flex items-center gap-2">
                  <Terminal size={14} />
                  手动安装步骤
                  {installError && <span className="text-error">（自动安装失败）</span>}
                </span>
                <ChevronDown
                  size={15}
                  className={clsx("transition-transform", showManual && "rotate-180")}
                />
              </button>
              {showManual && (
                <div className="divide-y divide-hairline border-t border-hairline">
                  <DesktopInstall platform={platformName} />
                </div>
              )}
              {installError && (
                <div className="border-t border-hairline px-[clamp(17px,3vw,24px)] py-3">
                  <pre className="max-h-28 overflow-auto whitespace-pre-wrap rounded-thumb bg-ground p-2.5 font-mono text-[11px] leading-relaxed text-error">
                    {installError}
                  </pre>
                </div>
              )}
            </div>
          )}

          {isMobile && platformName && (
            <div className="divide-y divide-hairline">
              <MobileGuide platform={platformName} />
            </div>
          )}
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
