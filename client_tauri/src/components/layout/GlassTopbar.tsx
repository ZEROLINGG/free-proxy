import clsx from "clsx";
import { useProxy } from "../../store/proxy";
import { useUi, type ViewId } from "../../store/ui";

const titles: Record<ViewId, string> = {
  dashboard: "仪表盘",
  proxy: "代理设置",
  speed: "IP 优选测速",
  ca: "CA 证书",
  about: "关于",
};

/** 玻璃顶栏：仅内容滚过 8px 后才出现 hairline 分隔。 */
export function GlassTopbar({ scrolled }: { scrolled: boolean }) {
  const view = useUi((s) => s.view);
  const running = useProxy((s) => s.status.running);

  return (
    <header
      className={clsx(
        "glass sticky top-0 z-40 border-b transition-[border-color,box-shadow] duration-300",
          "pt-[env(safe-area-inset-top)]",
          scrolled ? "border-hairline shadow-[0_1px_12px_rgba(0,0,0,0.04)]" : "border-transparent",
      )}
    >
      <div className="mx-auto flex h-13 max-w-180 items-center justify-between px-5.5">
        <h1 className="text-[17px] font-bold tracking-[-0.02em] text-ink">
          {titles[view]}
        </h1>
        <span className="hidden items-center gap-2 text-[12.5px] font-medium text-ink3 sm:flex">
          <span
            className={clsx(
              "h-2 w-2 rounded-full",
              running ? "pulse-dot" : "bg-faint",
            )}
          />
          {running ? "代理运行中" : "代理未运行"}
        </span>
      </div>
    </header>
  );
}
