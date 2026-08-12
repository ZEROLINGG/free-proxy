import {
  Gauge,
  Info,
  LayoutDashboard, PlaneTakeoff,
  Settings,
  ShieldCheck,
} from "lucide-react";
import clsx from "clsx";
import { useState } from "react";
import { useUi, type ViewId } from "../../store/ui";

const items: { id: ViewId; label: string; icon: typeof Gauge }[] = [
  { id: "dashboard", label: "仪表盘", icon: LayoutDashboard },
  { id: "proxy", label: "代理", icon: Settings },
  { id: "speed", label: "测速", icon: Gauge },
  { id: "ca", label: "CA 证书", icon: ShieldCheck },
  { id: "about", label: "关于", icon: Info },
];

/** 桌面侧边栏或移动端底部栏：实色 + hairline，悬停展开。选中 = 灰 pill + accent 图标。 */
export function Sidebar() {
  const view = useUi((s) => s.view);
  const navigate = useUi((s) => s.navigate);
  const navLocked = useUi((s) => s.navLocked);
  const [expanded, setExpanded] = useState(false);

  return (
    <nav
      aria-label="主导航"
      onMouseEnter={() => setExpanded(true)}
      onMouseLeave={() => setExpanded(false)}
      className={clsx(
        "flex shrink-0 flex-col gap-1 border-r md:border-r-0 border-hairline bg-ground py-3 transition-[width] duration-200 ease-out",
        expanded ? "w-44" : "w-14",
      )}
    >
      <div className="mx-2 mb-2 flex h-9 items-center gap-2.5 px-2">
        <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-[7px] bg-[linear-gradient(135deg,#0572DF,#32FBFB)]">
          <PlaneTakeoff size={18} className="text-gray-100"/>
        </div>
        {expanded && (
          <span className="truncate text-[13.5px] font-semibold tracking-tight text-ink">
            free-proxy
          </span>
        )}
      </div>

      {items.map(({ id, label, icon: Icon }) => {
        const active = view === id;
        return (
          <button
            key={id}
            type="button"
            aria-current={active ? "page" : undefined}
            title={expanded ? undefined : label}
            onClick={() => navigate(id)}
            className={clsx(
              "press mx-2 flex h-10 items-center gap-2.5 rounded-pill px-2.5 text-[13.5px] font-medium",
              active ? "bg-track text-ink" : "text-ink3 hover:bg-hover hover:text-ink",
              navLocked && !active && "opacity-40",
            )}
          >
            <Icon
              size={20}
              strokeWidth={1.75}
              className={clsx("shrink-0", active && "text-accent")}
            />
            {expanded && <span className="truncate">{label}</span>}
          </button>
        );
      })}
    </nav>
  );
}
