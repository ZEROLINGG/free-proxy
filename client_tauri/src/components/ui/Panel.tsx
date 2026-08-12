import clsx from "clsx";
import type { ReactNode } from "react";

/** 统一白面板：兄弟条目放同一面板，hairline 分隔，不堆碎片卡片。 */
export function Panel({
  children,
  className,
  title,
  right,
}: {
  children: ReactNode;
  className?: string;
  title?: string;
  right?: ReactNode;
}) {
  return (
    <section
      className={clsx(
        "overflow-hidden rounded-panel bg-surface shadow-panel",
        className,
      )}
    >
      {(title || right) && (
        <header className="flex items-center justify-between gap-3 px-[clamp(17px,3vw,24px)] pt-4">
          {title && (
            <h3 className="text-[13px] font-semibold tracking-wide text-ink3">
              {title}
            </h3>
          )}
          {right}
        </header>
      )}
      {children}
    </section>
  );
}

/** 面板内的分隔行（用 divide-y 分隔的相邻行）。 */
export function ListRow({
  children,
  onClick,
  className,
}: {
  children: ReactNode;
  onClick?: () => void;
  className?: string;
}) {
  return (
    <div
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
      onClick={onClick}
      onKeyDown={
        onClick
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onClick();
              }
            }
          : undefined
      }
      className={clsx(
        "flex min-h-[56px] items-center gap-3 px-[clamp(17px,3vw,24px)] py-3.5",
        onClick && "row-hover cursor-pointer",
        className,
      )}
    >
      {children}
    </div>
  );
}
