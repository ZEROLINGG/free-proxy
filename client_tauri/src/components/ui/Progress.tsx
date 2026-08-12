import clsx from "clsx";

export function Progress({
  value,
  indeterminate,
  className,
}: {
  /** 0–100；indeterminate 时忽略 */
  value?: number;
  indeterminate?: boolean;
  className?: string;
}) {
  return (
    <div
      role="progressbar"
      aria-valuenow={indeterminate ? undefined : value}
      className={clsx(
        "relative h-1.5 w-full overflow-hidden rounded-full bg-track",
        className,
      )}
    >
      {indeterminate ? (
        <span className="absolute inset-y-0 w-1/3 animate-[indeterminate_1.4s_ease-in-out_infinite] rounded-full bg-accent" />
      ) : (
        <span
          className="block h-full rounded-full bg-accent transition-[width] duration-300 ease-out"
          style={{ width: `${Math.max(0, Math.min(100, value ?? 0))}%` }}
        />
      )}
    </div>
  );
}
