import { Eye, EyeOff } from "lucide-react";
import clsx from "clsx";
import { useState, type InputHTMLAttributes } from "react";

interface Props extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  hint?: string;
  error?: string;
  mono?: boolean;
}

/** 面板内嵌输入：hairline 底边，聚焦时 accent 底线。 */
export function Input({ label, hint, error, mono, className, type, id, ...rest }: Props) {
  const [show, setShow] = useState(false);
  const isPassword = type === "password";
  const inputId = id ?? (label ? `input-${label}` : undefined);

  return (
    <div className="min-w-0 flex-1">
      {label && (
        <label
          htmlFor={inputId}
          className="mb-1 block text-[12.5px] font-medium text-ink2"
        >
          {label}
        </label>
      )}
      <div
        className={clsx(
          "flex items-center gap-2 border-b border-hairline transition-colors duration-150 focus-within:border-accent",
          error && "border-error focus-within:border-error",
        )}
      >
        <input
          id={inputId}
          type={isPassword && show ? "text" : type ?? "text"}
          className={clsx(
            "h-11 w-full min-w-0 bg-transparent text-[15px] text-ink placeholder:text-ink4 focus:outline-none",
            mono && "font-mono text-[13.5px]",
            className,
          )}
          {...rest}
        />
        {isPassword && (
          <button
            type="button"
            tabIndex={-1}
            aria-label={show ? "隐藏密钥" : "显示密钥"}
            onClick={() => setShow((v) => !v)}
            className="press flex h-9 w-9 shrink-0 items-center justify-center text-ink3 hover:text-ink"
          >
            {show ? <EyeOff size={17} /> : <Eye size={17} />}
          </button>
        )}
      </div>
      {error ? (
        <p className="mt-1.5 text-[12px] text-error" role="alert">
          {error}
        </p>
      ) : hint ? (
        <p className="mt-1.5 text-[12px] text-ink3">{hint}</p>
      ) : null}
    </div>
  );
}
