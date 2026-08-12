import { Check, ChevronDown } from "lucide-react";
import clsx from "clsx";
import {
  Children,
  isValidElement,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useEffect } from "react";

interface Props {
  label?: string;
  value?: string;
  disabled?: boolean;
  className?: string;
  "aria-label"?: string;
  onChange?: (e: { target: { value: string } }) => void;
  children?: ReactNode;
}

interface Option {
  value: string;
  label: string;
}

const PANEL_MAX_HEIGHT = 280;
const OPTION_HEIGHT = 36;

/** 自定义下拉：原生 select 的弹出列表由 OS 渲染、无法贴合应用风格，
 *  这里用 portal + fixed 浮层完全接管外观与交互（含键盘导航）。 */
export function Select({
  label,
  value = "",
  disabled,
  className,
  "aria-label": ariaLabel,
  onChange,
  children,
}: Props) {
  const options = useMemo(() => {
    const list: Option[] = [];
    Children.forEach(children, (child) => {
      if (isValidElement(child)) {
        const props = child.props as { value?: unknown; children?: unknown };
        if (props.value !== undefined) {
          list.push({ value: String(props.value), label: String(props.children ?? "") });
        }
      }
    });
    return list;
  }, [children]);

  const [open, setOpen] = useState(false);
  const [activeIdx, setActiveIdx] = useState(-1);
  const [pos, setPos] = useState<{ top: number; left: number; width: number; openUp: boolean } | null>(
    null,
  );
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  const selectedLabel = options.find((o) => o.value === value)?.label ?? value;

  const select = (v: string) => {
    setOpen(false);
    if (v !== value) onChange?.({ target: { value: v } });
  };

  // 打开时测量触发按钮位置；底部溢出则向上翻转
  useLayoutEffect(() => {
    if (!open || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    const panelH = Math.min(options.length * OPTION_HEIGHT + 8, PANEL_MAX_HEIGHT);
    const openUp = rect.bottom + panelH + 8 > window.innerHeight && rect.top - panelH - 8 > 0;
    setPos({
      top: openUp ? rect.top - panelH - 4 : rect.bottom + 4,
      left: rect.left,
      width: rect.width,
      openUp,
    });
    setActiveIdx(Math.max(0, options.findIndex((o) => o.value === value)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // 外部点击关闭
  useEffect(() => {
    if (!open) return;
    const onDocDown = (e: MouseEvent | TouchEvent) => {
      const t = e.target as Node;
      if (triggerRef.current?.contains(t) || panelRef.current?.contains(t)) return;
      setOpen(false);
    };
    document.addEventListener("mousedown", onDocDown);
    document.addEventListener("touchstart", onDocDown);
    return () => {
      document.removeEventListener("mousedown", onDocDown);
      document.removeEventListener("touchstart", onDocDown);
    };
  }, [open]);

  // 滚动 / 缩放时关闭（fixed 坐标会失效）
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [open]);

  // 高亮项滚入可视区
  useEffect(() => {
    if (!open || activeIdx < 0) return;
    const el = panelRef.current?.querySelector<HTMLElement>(`[data-idx="${activeIdx}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [activeIdx, open]);

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (disabled) return;
    if (!open) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) {
        e.preventDefault();
        setOpen(true);
        setActiveIdx((i) =>
          i === -1 ? Math.max(0, options.findIndex((o) => o.value === value)) : i,
        );
      }
      return;
    }
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setActiveIdx((i) => (i + 1) % options.length);
        break;
      case "ArrowUp":
        e.preventDefault();
        setActiveIdx((i) => (i - 1 + options.length) % options.length);
        break;
      case "Home":
        e.preventDefault();
        setActiveIdx(0);
        break;
      case "End":
        e.preventDefault();
        setActiveIdx(options.length - 1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (activeIdx >= 0 && options[activeIdx]) select(options[activeIdx].value);
        break;
      case "Escape":
        e.preventDefault();
        setOpen(false);
        break;
      case "Tab":
        setOpen(false);
        break;
    }
  };

  return (
    <div className="min-w-0 flex-1">
      {label && (
        <span className="mb-1 block text-[12.5px] font-medium text-ink2">{label}</span>
      )}
      <div className="relative">
        <button
          ref={triggerRef}
          type="button"
          disabled={disabled}
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-label={ariaLabel}
          onKeyDown={onKeyDown}
          onClick={() => setOpen((v) => !v)}
          className={clsx(
            "h-11 w-full cursor-pointer rounded-thumb border border-hairline bg-surface pl-3.5 pr-9 text-left text-[14px] text-ink shadow-card transition-colors duration-150 hover:bg-hover focus:outline-none focus-visible:border-accent disabled:cursor-not-allowed disabled:opacity-50",
            open && "border-accent",
            className,
          )}
        >
          <span className="block truncate">{selectedLabel}</span>
        </button>
        <ChevronDown
          size={16}
          className={clsx(
            "pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-ink3 transition-transform duration-150",
            open && "rotate-180",
          )}
        />
      </div>

      {open &&
        pos &&
        createPortal(
          <div
            ref={panelRef}
            role="listbox"
            aria-label={ariaLabel}
            style={{ top: pos.top, left: pos.left, width: pos.width, maxHeight: PANEL_MAX_HEIGHT }}
            className={clsx(
              "fixed z-50 overflow-auto rounded-thumb border border-hairline bg-surface py-1 shadow-panel",
              pos.openUp ? "rounded-b-none" : "rounded-t-none",
            )}
          >
            {options.map((o, i) => {
              const selected = o.value === value;
              return (
                <button
                  key={o.value}
                  type="button"
                  role="option"
                  aria-selected={selected}
                  data-idx={i}
                  title={o.label}
                  onClick={() => select(o.value)}
                  onMouseEnter={() => setActiveIdx(i)}
                  className={clsx(
                    "flex h-9 w-full items-center gap-2 px-3 text-left text-[14px]",
                    selected ? "bg-accent/5 text-accent-link" : "text-ink",
                    i === activeIdx && !selected && "bg-hover",
                  )}
                >
                  <span className="min-w-0 flex-1 truncate">{o.label}</span>
                  {selected && <Check size={14} className="shrink-0" />}
                </button>
              );
            })}
          </div>,
          document.body,
        )}
    </div>
  );
}
