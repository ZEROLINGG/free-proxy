import {
  Gauge,
  Info,
  LayoutDashboard,
  Settings,
  ShieldCheck,
} from "lucide-react";
import clsx from "clsx";
import { useUi, type ViewId } from "../../store/ui";

const tabs: { id: ViewId; label: string; icon: typeof Gauge }[] = [
  { id: "dashboard", label: "仪表盘", icon: LayoutDashboard },
  { id: "proxy", label: "代理", icon: Settings },
  { id: "speed", label: "测速", icon: Gauge },
  { id: "ca", label: "CA 证书", icon: ShieldCheck },
  { id: "about", label: "关于", icon: Info },
];

/** 移动端底部 Tab bar：玻璃 + 安全区，激活项 accent（line → filled）。 */
export function BottomTabs() {
  const view = useUi((s) => s.view);
  const navigate = useUi((s) => s.navigate);
  const navLocked = useUi((s) => s.navLocked);

  return (
    <nav
      aria-label="主导航"
      role="tablist"
      className="glass flex-none border-t border-hairline"
      style={{ paddingBottom: "max(8px, env(safe-area-inset-bottom))" }}
    >
      <div className="flex items-start justify-around">
        {tabs.map(({ id, label, icon: Icon }) => {
          const active = view === id;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => navigate(id)}
              className={clsx(
                "press flex w-16 select-none flex-col items-center gap-0.5 py-1.5",
                active ? "text-accent" : "text-ink3",
                navLocked && !active && "opacity-40",
              )}
            >
              <Icon
                size={26}
                strokeWidth={1.75}
                fill={active ? "currentColor" : "none"}
              />
              <span className="text-[10px] font-medium">{label}</span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
