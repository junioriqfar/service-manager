import type { BatchSummary, ServiceConfig, ServiceRow } from "./types";

export function typeLabel(type: ServiceConfig["serviceType"]): string {
  if (type === "apache") return "Apache";
  if (type === "postgresql") return "PostgreSQL";
  return "NSSM";
}

export function startupLabel(startup: ServiceConfig["startup"]): string {
  if (startup === "delayed") return "Auto (Delayed)";
  if (startup === "manual") return "Manual";
  return "Automatic";
}

export function restartLabel(service: Pick<ServiceRow, "restartOnCrash" | "restartDelaySec">): string {
  return service.restartOnCrash ? `Yes (${service.restartDelaySec}s)` : "No";
}

export function statusVariant(
  state: string,
): "success" | "destructive" | "warning" | "secondary" | "outline" {
  switch (state) {
    case "running":
      return "success";
    case "stopped":
      return "destructive";
    case "notInstalled":
      return "secondary";
    case "startPending":
    case "stopPending":
    case "continuePending":
    case "pausePending":
    case "paused":
      return "warning";
    default:
      return "outline";
  }
}

export function countsLabel(services: ServiceRow[]): string {
  const installed = services.filter((service) => service.installed).length;
  const running = services.filter((service) => service.state === "running").length;
  return `${services.length} registered | ${installed} installed | ${running} running`;
}

export function batchSummaryText(summary: BatchSummary): string {
  return `Completed: ${summary.succeeded} succeeded, ${summary.failed} failed, ${summary.cancelled} cancelled.`;
}

export function splitDependencies(value: string): string[] {
  return value
    .split(/[\s,]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}
