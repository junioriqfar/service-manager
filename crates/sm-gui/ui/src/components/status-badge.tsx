import type { LucideIcon } from "lucide-react";
import {
  Activity,
  AlertTriangle,
  Ban,
  CheckCircle2,
  Circle,
  CircleCheck,
  Download,
  HelpCircle,
  Loader2,
  PauseCircle,
  PlayCircle,
  Search,
  ServerOff,
  ShieldAlert,
  Square,
  XCircle,
} from "lucide-react";

import { Hint } from "@/components/hint";
import { Badge, type BadgeProps } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

type StatusTone = NonNullable<BadgeProps["variant"]>;

export interface StatusBadgeProps {
  tone: StatusTone;
  label: string;
  hint: string;
  icon?: LucideIcon;
  spin?: boolean;
  className?: string;
}

/**
 * Icon + colored badge + tooltip. The hint must explain what the status means
 * operationally instead of repeating the label.
 */
export function StatusBadge({
  tone,
  label,
  hint,
  icon: Icon,
  spin = false,
  className,
}: StatusBadgeProps) {
  return (
    <Hint label={hint}>
      <Badge
        variant={tone}
        tabIndex={0}
        className={cn(
          "max-w-full gap-1 whitespace-nowrap px-2 py-0.5 text-[11px] font-semibold",
          className,
        )}
      >
        {Icon ? (
          <Icon
            className={cn("h-3 w-3 shrink-0", spin && "animate-spin")}
            aria-hidden="true"
          />
        ) : null}
        {label}
      </Badge>
    </Hint>
  );
}

export interface ServiceStatusDescriptor {
  tone: StatusTone;
  label: string;
  hint: string;
  icon: LucideIcon;
  spin?: boolean;
}

export function serviceStatusDescriptor(
  state: string,
  label: string,
): ServiceStatusDescriptor {
  switch (state) {
    case "running":
      return {
        tone: "success",
        label,
        icon: PlayCircle,
        hint: "Windows reports the service as Running; it is accepting work.",
      };
    case "stopped":
      return {
        tone: "destructive",
        label,
        icon: Square,
        hint: "The service is installed but stopped; start it to restore its workload.",
      };
    case "paused":
      return {
        tone: "warning",
        label,
        icon: PauseCircle,
        hint: "The service is paused by the SCM; resume or restart it before updating.",
      };
    case "startPending":
    case "stopPending":
    case "continuePending":
    case "pausePending":
      return {
        tone: "warning",
        label,
        icon: Loader2,
        spin: true,
        hint: "The SCM is applying a state change; wait for the service to settle before acting.",
      };
    case "notInstalled":
      return {
        tone: "secondary",
        label,
        icon: ServerOff,
        hint: "This config is not installed as a Windows service yet; install it first.",
      };
    default:
      return {
        tone: "outline",
        label,
        icon: HelpCircle,
        hint: "The SCM status could not be read; refresh the list or check the service manually.",
      };
  }
}

export function ServiceStatusBadge({
  state,
  label,
  pending = false,
  hint,
}: {
  state: string;
  label: string;
  pending?: boolean;
  hint?: string;
}) {
  const descriptor = serviceStatusDescriptor(state, label);
  return (
    <StatusBadge
      tone={descriptor.tone}
      label={descriptor.label}
      hint={hint || descriptor.hint}
      icon={pending ? Loader2 : descriptor.icon}
      spin={pending || descriptor.spin}
    />
  );
}

export interface UpdaterStatusDescriptor {
  tone: StatusTone;
  label: string;
  hint: string;
  icon: LucideIcon;
}

export function updaterStatusDescriptor(status: string): UpdaterStatusDescriptor {
  switch (status) {
    case "updated":
      return {
        tone: "success",
        label: status,
        icon: CheckCircle2,
        hint: "The last run pulled and applied the update successfully.",
      };
    case "up-to-date":
      return {
        tone: "secondary",
        label: status,
        icon: CircleCheck,
        hint: "No incoming commits; there is nothing to update for this app.",
      };
    case "check":
      return {
        tone: "outline",
        label: status,
        icon: Search,
        hint: "Check-only result; no files, dependencies, or services were changed.",
      };
    case "ready":
      return {
        tone: "success",
        label: status,
        icon: CheckCircle2,
        hint: "Remote changes are available and preflight checks passed.",
      };
    case "dirty":
      return {
        tone: "warning",
        label: status,
        icon: AlertTriangle,
        hint: "The working tree has changes not covered by dirtyIgnore; updates are blocked.",
      };
    case "blocked-migration":
      return {
        tone: "warning",
        label: status,
        icon: ShieldAlert,
        hint: "Migration files changed; automatic updates stop until migrations are handled manually.",
      };
    case "clone-required":
      return {
        tone: "warning",
        label: status,
        icon: Download,
        hint: "The local repository is missing; enable Clone on first run or clone it manually.",
      };
    case "degraded":
      return {
        tone: "warning",
        label: status,
        icon: Activity,
        hint: "The update finished but the health check or a service step reported a problem.",
      };
    case "failed":
      return {
        tone: "destructive",
        label: status,
        icon: XCircle,
        hint: "The last run failed; review the app log before retrying.",
      };
    case "forbidden":
      return {
        tone: "destructive",
        label: status,
        icon: Ban,
        hint: "The run was rejected by the IP allowlist or maintenance policy; no files were changed.",
      };
    case "error":
      return {
        tone: "destructive",
        label: status,
        icon: XCircle,
        hint: "The last run ended with an unexpected error; review the log for details.",
      };
    case "idle":
      return {
        tone: "outline",
        label: status,
        icon: Circle,
        hint: "No update or check result has been recorded for this app yet.",
      };
    default:
      return {
        tone: "outline",
        label: status || "unknown",
        icon: Circle,
        hint: "No status detail is available for this app yet.",
      };
  }
}

export function UpdaterStatusBadge({
  status,
  hint,
}: {
  status: string;
  hint?: string;
}) {
  const descriptor = updaterStatusDescriptor(status);
  return (
    <StatusBadge
      tone={descriptor.tone}
      label={descriptor.label}
      hint={hint || descriptor.hint}
      icon={descriptor.icon}
    />
  );
}
