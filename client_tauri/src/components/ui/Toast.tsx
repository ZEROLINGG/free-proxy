import { CheckCircle2, Info, X, XCircle } from "lucide-react";
import clsx from "clsx";
import { useUi } from "../../store/ui";

const icons = {
  success: CheckCircle2,
  error: XCircle,
  info: Info,
};

const colors = {
  success: "text-[#1d9a45] dark:text-[#3ddc68]",
  error: "text-error",
  info: "text-accent",
};

export function ToastViewport() {
  const toasts = useUi((s) => s.toasts);
  const dismiss = useUi((s) => s.dismiss);

  return (
    <div
      aria-live="polite"
      className="pointer-events-none fixed right-4 top-16 z-[100] flex w-[min(92vw,380px)] flex-col gap-2 md:right-6 md:top-auto md:bottom-6"
    >
      {toasts.map((t) => {
        const Icon = icons[t.kind];
        return (
          <div
            key={t.id}
            className="toast-in pointer-events-auto flex items-start gap-2.5 rounded-card border border-hairline bg-surface/95 px-4 py-3 shadow-overlay backdrop-blur-xl"
          >
            <Icon size={18} className={clsx("mt-0.5 shrink-0", colors[t.kind])} />
            <p className="flex-1 text-[13.5px] leading-snug text-ink break-words">
              {t.text}
            </p>
            <button
              type="button"
              aria-label="关闭"
              onClick={() => dismiss(t.id)}
              className="press -mr-1 -mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-ink3 hover:text-ink"
            >
              <X size={15} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
