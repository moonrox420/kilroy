/**
 * Toaster — bottom-right stack of dismissable notification cards.
 *
 * Reads from the notifications store. Each card auto-dismisses after its
 * `durationMs` (managed in the store). The X button is a manual dismiss.
 *
 * Colour-coded by kind: amber for warn, green-ok for success, red-err
 * for error, neutral for info. Sticks below the status bar so it doesn't
 * cover the chat scroll-anchored input.
 */
import { TriangleAlert, CircleCheck, Info, X, CircleX } from "lucide-react";
import { useNotifications, type Toast } from "@/store/notifications";
import { cn } from "@/lib/utils";

export function Toaster() {
  const toasts = useNotifications((s) => s.toasts);
  const dismiss = useNotifications((s) => s.dismiss);

  if (toasts.length === 0) return null;

  return (
    <div
      className="pointer-events-none fixed right-3 z-50 flex flex-col items-end gap-2"
      style={{ bottom: "calc(var(--statusbar-h) + 12px)" }}
      aria-live="polite"
      aria-atomic="false"
    >
      {toasts.map((t) => (
        <ToastCard key={t.id} toast={t} onDismiss={() => dismiss(t.id)} />
      ))}
    </div>
  );
}

function ToastCard({
  toast,
  onDismiss,
}: {
  toast: Toast;
  onDismiss: () => void;
}) {
  const Icon =
    toast.kind === "error"
      ? CircleX
      : toast.kind === "warn"
        ? TriangleAlert
        : toast.kind === "success"
          ? CircleCheck
          : Info;

  const palette =
    toast.kind === "error"
      ? "border-err/40 bg-err/10 text-ink"
      : toast.kind === "warn"
        ? "border-warn/40 bg-warn/10 text-ink"
        : toast.kind === "success"
          ? "border-ok/40 bg-ok/10 text-ink"
          : "border-line bg-bg-1 text-ink";

  const iconColor =
    toast.kind === "error"
      ? "text-err"
      : toast.kind === "warn"
        ? "text-warn"
        : toast.kind === "success"
          ? "text-ok"
          : "text-ink-subtle";

  return (
    <div
      className={cn(
        "pointer-events-auto flex max-w-[420px] items-start gap-2 rounded-md border px-3 py-2 shadow-md",
        "animate-in slide-in-from-right-4 fade-in duration-200",
        palette,
      )}
      role={toast.kind === "error" ? "alert" : "status"}
    >
      <Icon className={cn("mt-0.5 h-4 w-4 shrink-0", iconColor)} />
      <div className="min-w-0 flex-1">
        <p className="text-[12px] font-medium leading-snug">{toast.title}</p>
        {toast.detail && (
          <p className="mt-0.5 break-words text-[11px] leading-snug text-ink-muted">
            {toast.detail}
          </p>
        )}
      </div>
      <button
        onClick={onDismiss}
        className="rounded-sm p-0.5 text-ink-subtle hover:bg-bg-2 hover:text-ink"
        aria-label="Dismiss notification"
      >
        <X className="h-3 w-3" />
      </button>
    </div>
  );
}
