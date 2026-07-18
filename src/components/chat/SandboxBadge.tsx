/**
 * Sandbox badge — small pill on action card headers indicating where the
 * action will execute. Most important on shell actions, where the
 * difference between "Host" (touches your machine) and "Windows Sandbox"
 * (disposable VM) is the difference between "safe" and "destructive
 * potential". Make that loud at a glance, not just in expanded detail.
 */
import { Shield, ShieldOff, Box } from "lucide-react";
import { cn } from "@/lib/utils";

export type SandboxId = "host" | "windows_sandbox" | "docker" | string;

interface Props {
  sandbox: SandboxId;
  className?: string;
}

export function SandboxBadge({ sandbox, className }: Props) {
  const meta = describeSandbox(sandbox);
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-sm px-1.5 py-[1px] text-[9px] uppercase tracking-wider",
        meta.cls,
        className,
      )}
      title={meta.tooltip}
    >
      <meta.Icon className="h-2.5 w-2.5" />
      {meta.label}
    </span>
  );
}

function describeSandbox(id: SandboxId): {
  label: string;
  cls: string;
  tooltip: string;
  Icon: typeof Shield;
} {
  switch (id) {
    case "windows_sandbox":
      return {
        label: "sandboxed",
        cls: "bg-ok/15 text-ok",
        tooltip:
          "Runs inside a Windows Sandbox (disposable Hyper-V VM). Your host machine is not affected.",
        Icon: Shield,
      };
    case "docker":
      return {
        label: "docker",
        cls: "bg-amber/15 text-amber",
        tooltip: "Runs inside a Docker container.",
        Icon: Box,
      };
    case "host":
      return {
        label: "host",
        cls: "bg-warn/20 text-warn",
        tooltip:
          "Runs DIRECTLY on your machine — no isolation. Review the command carefully before accepting.",
        Icon: ShieldOff,
      };
    default:
      return {
        label: id,
        cls: "bg-line text-ink-subtle",
        tooltip: `Sandbox: ${id}`,
        Icon: Box,
      };
  }
}
