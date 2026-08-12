import clsx from "clsx";
import type { ButtonHTMLAttributes } from "react";
import { Spinner } from "./Spinner";

type Variant = "primary" | "secondary" | "danger" | "ghost";

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  loading?: boolean;
}

const variants: Record<Variant, string> = {
  primary: "bg-accent text-white hover:brightness-110",
  secondary:
    "bg-surface text-ink border border-hairline shadow-card hover:bg-hover",
  danger: "bg-error-bg text-error",
  ghost: "text-accent hover:bg-accent/10",
};

export function Button({
  variant = "primary",
  loading,
  disabled,
  className,
  children,
  ...rest
}: Props) {
  return (
    <button
      className={clsx(
        "press inline-flex h-11 select-none items-center justify-center gap-2 whitespace-nowrap rounded-pill px-5 text-[15px] font-semibold disabled:pointer-events-none disabled:opacity-50",
        variants[variant],
        className,
      )}
      disabled={disabled || loading}
      {...rest}
    >
      {loading && <Spinner className="h-4 w-4" />}
      {children}
    </button>
  );
}
