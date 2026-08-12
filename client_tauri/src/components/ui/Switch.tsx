import clsx from "clsx";

interface Props {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  label?: string;
  className?: string;
}

/** iOS 风格开关：视觉 28px 高，整体点击区 44px。 */
export function Switch({ checked, onChange, disabled, label, className }: Props) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={clsx(
        "press relative h-7 w-11 shrink-0 rounded-pill transition-colors duration-200",
        checked ? "bg-accent" : "bg-track",
        disabled && "pointer-events-none opacity-50",
        className,
      )}
    >
      <span
        className={clsx(
          "absolute top-0.5 left-0.5 h-6 w-6 rounded-full bg-white shadow-md transition-transform duration-200 ease-out",
          checked && "translate-x-4",
        )}
      />
    </button>
  );
}
