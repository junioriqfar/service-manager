import { useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  ArrowRightLeft,
  CheckCircle2,
  ClipboardCopy,
  Clock,
  FlaskConical,
  FolderOpen,
  Loader2,
  Network,
  Play,
  RefreshCw,
  Save,
  Search,
  Server,
  ShieldAlert,
  ShieldCheck,
  Stethoscope,
  Terminal,
  Trash2,
  Wrench,
  XCircle,
} from "lucide-react";
import { toast } from "sonner";

import { Hint } from "@/components/hint";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { errorMessage, invoke } from "@/lib/tauri";
import type {
  ApacheApplyResult,
  ApacheChangeView,
  ApacheMappingScanView,
  ApacheToolPath,
  DoctorCheck,
  DoctorReport,
  DoctorStatus,
  PhpPathChangeView,
  ServiceRow,
  Settings,
  SettingsDiagnostics,
  SettingsSaveResult,
  SettingsTools,
  SettingsTrigger,
  SettingsView,
  TaskOperation,
  TaskStatus,
  ToolDetection,
  ToolPath,
  ToolTest,
  UpdaterApp,
  UpdateTimeouts,
} from "@/lib/types";
import { cn } from "@/lib/utils";

type ToolKey = keyof SettingsTools;
type ToolBusy = "browse" | "detect" | "test" | null;
type TaskAction = "install" | "remove" | "test";

interface ToolDefinition {
  key: ToolKey;
  label: string;
  required: boolean;
  description: string;
}

interface ToolMessage {
  text: string;
  tone: "ok" | "error" | "info";
}

const TOOL_DEFINITIONS: ToolDefinition[] = [
  {
    key: "git",
    label: "Git",
    required: true,
    description: "Git executable used for repository checks and pulls.",
  },
  {
    key: "php",
    label: "PHP",
    required: true,
    description: "PHP CLI used by Laravel commands and Composer. App commands may use ${PHP} and ${PHP_HOME}.",
  },
  {
    key: "composer",
    label: "Composer",
    required: true,
    description: "Composer launcher used for PHP dependencies.",
  },
  {
    key: "node",
    label: "Node.js",
    required: false,
    description: "Node.js runtime used by JavaScript projects.",
  },
  {
    key: "npm",
    label: "npm",
    required: true,
    description: "npm launcher used for Node.js dependencies.",
  },
  {
    key: "pnpm",
    label: "pnpm",
    required: false,
    description: "Optional pnpm package manager.",
  },
  {
    key: "yarn",
    label: "Yarn",
    required: false,
    description: "Optional Yarn package manager.",
  },
  {
    key: "bun",
    label: "Bun",
    required: false,
    description: "Bun runtime and package manager.",
  },
  {
    key: "go",
    label: "Go",
    required: false,
    description: "Go toolchain used for Go projects.",
  },
  {
    key: "apache",
    label: "Apache",
    required: false,
    description: "Apache httpd executable, root folder, and config file.",
  },
  {
    key: "nssm",
    label: "NSSM",
    required: false,
    description: "NSSM executable used to manage Windows services.",
  },
];

const TIMEOUT_FIELDS: Array<{
  key: keyof UpdateTimeouts;
  label: string;
  hint: string;
}> = [
  {
    key: "preflightSeconds",
    label: "Preflight timeout",
    hint: "Maximum seconds for preflight checks before an update starts.",
  },
  {
    key: "fetchSeconds",
    label: "Fetch timeout",
    hint: "Maximum seconds for Git fetch and remote inspection.",
  },
  {
    key: "toolSeconds",
    label: "Tool timeout",
    hint: "Maximum seconds for package manager and build commands.",
  },
  {
    key: "cloneSeconds",
    label: "Clone timeout",
    hint: "Maximum seconds for cloning a project on first run.",
  },
];

function toolLabel(key: ToolKey): string {
  return TOOL_DEFINITIONS.find((definition) => definition.key === key)?.label ?? key;
}

function formatWhen(value: string): string {
  if (!value) return "—";
  return value.replace("T", " ").slice(0, 19);
}

function splitPatterns(value: string): string[] {
  return value
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function sameTrigger(left: SettingsTrigger, right: SettingsTrigger): boolean {
  return (
    left.allowLocal === right.allowLocal &&
    left.allowedClientIPs.length === right.allowedClientIPs.length &&
    left.allowedClientIPs.every((value, index) => value === right.allowedClientIPs[index])
  );
}

function inferApache(executable: string): { root: string; config: string } {
  const parts = executable.split(/[\\/]/);
  if (parts.length >= 3 && parts[parts.length - 2]?.toLowerCase() === "bin") {
    return {
      root: parts.slice(0, -2).join("\\"),
      config: "conf/httpd.conf",
    };
  }
  return {
    root: parts.slice(0, -1).join("\\"),
    config: "",
  };
}

function toolBadge(
  value: ToolPath,
  message: ToolMessage | undefined,
  busy: ToolBusy,
): { variant: "success" | "warning" | "destructive" | "outline"; label: string; title: string } {
  if (busy === "detect") return { variant: "outline", label: "Detecting…", title: "" };
  if (busy === "test") return { variant: "outline", label: "Testing…", title: "" };
  if (busy === "browse") return { variant: "outline", label: "Browsing…", title: "" };
  if (!value.path.trim()) {
    return { variant: "warning", label: "Not set", title: "No path configured" };
  }
  if (message?.tone === "error") {
    return { variant: "destructive", label: "Check failed", title: message.text };
  }
  if (value.version.trim()) {
    return { variant: "success", label: value.version.trim(), title: value.version.trim() };
  }
  if (message?.tone === "ok") {
    return { variant: "success", label: "Ready", title: message.text };
  }
  return {
    variant: "outline",
    label: "Not tested",
    title: "Path configured. Run Test to verify it.",
  };
}

function taskBadge(
  status: TaskStatus | null,
  error: string | null,
): { variant: "success" | "warning" | "destructive" | "outline"; label: string } {
  if (error) return { variant: "destructive", label: "Unavailable" };
  if (!status) return { variant: "outline", label: "Unknown" };
  if (!status.installed) return { variant: "warning", label: "Not installed" };
  const state = status.state.toLowerCase();
  if (state === "ready" || state === "running") {
    return { variant: "success", label: status.state };
  }
  if (state === "disabled") return { variant: "warning", label: "Disabled" };
  return { variant: "outline", label: status.state || "Installed" };
}

function ToolCard({
  definition,
  value,
  busy,
  message,
  onChange,
  onBrowse,
  onDetect,
  onTest,
}: {
  definition: ToolDefinition;
  value: ToolPath & { root?: string; config?: string };
  busy: ToolBusy;
  message?: ToolMessage;
  onChange: (patch: Partial<ApacheToolPath>) => void;
  onBrowse: () => void;
  onDetect: () => void;
  onTest: () => void;
}) {
  const badge = toolBadge(value, message, busy);
  const disabled = busy !== null;

  return (
    <div className="rounded-lg border bg-background/40 p-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-semibold">{definition.label}</span>
            {definition.required && <Badge variant="outline">Required</Badge>}
          </div>
          <p className="text-xs text-muted-foreground">{definition.description}</p>
        </div>
        <Badge
          variant={badge.variant}
          className="max-w-56 truncate"
          title={badge.title || badge.label}
        >
          {badge.label}
        </Badge>
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Input
          value={value.path}
          onChange={(event) =>
            onChange({ path: event.target.value, version: "", detectedAt: "" })
          }
          placeholder="C:\\path\\to\\executable"
          className="h-8 min-w-0 flex-1 font-mono text-xs"
          title={`Path to ${definition.label}. You can type it, browse, or auto-detect.`}
          aria-label={`${definition.label} path`}
        />
        <Hint label={`Browse for the ${definition.label} executable`}>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={onBrowse}
            disabled={disabled}
          >
            {busy === "browse" ? <Loader2 className="animate-spin" /> : <FolderOpen />}
            Browse…
          </Button>
        </Hint>
        <Hint label={`Search the configured path, PATH, and common install locations for ${definition.label}`}>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={onDetect}
            disabled={disabled}
          >
            {busy === "detect" ? <Loader2 className="animate-spin" /> : <Search />}
            Auto-detect
          </Button>
        </Hint>
        <Hint label={`Run the ${definition.label} version command and show the result`}>
          <Button type="button" size="sm" onClick={onTest} disabled={disabled}>
            {busy === "test" ? <Loader2 className="animate-spin" /> : <FlaskConical />}
            Test
          </Button>
        </Hint>
      </div>

      {definition.key === "apache" && (
        <div className="mt-2 grid gap-2 sm:grid-cols-2">
          <div className="space-y-1">
            <Label className="text-xs" htmlFor="settings-apache-root">
              Apache root
            </Label>
            <Input
              id="settings-apache-root"
              value={value.root ?? ""}
              onChange={(event) => onChange({ root: event.target.value })}
              placeholder="C:\\laragon\\bin\\apache\\httpd-2.4"
              className="h-8 font-mono text-xs"
              title="Apache installation root; config is resolved relative to this folder."
            />
          </div>
          <div className="space-y-1">
            <Label className="text-xs" htmlFor="settings-apache-config">
              Apache config
            </Label>
            <Input
              id="settings-apache-config"
              value={value.config ?? ""}
              onChange={(event) => onChange({ config: event.target.value })}
              placeholder="conf/httpd.conf"
              className="h-8 font-mono text-xs"
              title="Apache config path, relative to the root or absolute."
            />
          </div>
        </div>
      )}

      {message && (
        <p
          className={cn(
            "mt-2 break-words text-xs",
            message.tone === "error" && "text-destructive",
            message.tone === "ok" && "text-emerald-600 dark:text-emerald-400",
            message.tone === "info" && "text-muted-foreground",
          )}
        >
          {message.text}
        </p>
      )}
    </div>
  );
}

function Diagnostics({
  diagnostics,
}: {
  diagnostics: SettingsDiagnostics | undefined;
}) {
  if (!diagnostics) {
    return (
      <p className="text-sm text-muted-foreground">
        Diagnostics are unavailable. Reload settings to try again.
      </p>
    );
  }

  const clean =
    diagnostics.missingRequiredTools.length === 0 &&
    diagnostics.invalidPaths.length === 0 &&
    diagnostics.taskInstalled &&
    !diagnostics.taskError;

  return (
    <div className="space-y-2 text-sm">
      {diagnostics.legacyFallback && (
        <div className="flex items-start gap-2 rounded-md border border-amber-300/70 bg-amber-50 px-3 py-2 text-amber-900 dark:border-amber-500/30 dark:bg-amber-950/40 dark:text-amber-200">
          <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" />
          <div>
            <div className="font-medium">Legacy fallback</div>
            <p className="text-xs">
              data/settings.json was not found. Values are derived from projects.json. Click Save
              Settings to create the settings file.
            </p>
          </div>
        </div>
      )}

      {diagnostics.missingRequiredTools.length > 0 && (
        <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-destructive">
          <XCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <div>
            <div className="font-medium">Missing required tools</div>
            <p className="text-xs">{diagnostics.missingRequiredTools.join(", ")}</p>
          </div>
        </div>
      )}

      {diagnostics.invalidPaths.map((issue) => (
        <div
          key={`${issue.tool}:${issue.path}`}
          className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-destructive"
        >
          <XCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <div>
            <div className="font-medium">Invalid tool path</div>
            <p className="break-all text-xs">{issue.message}</p>
          </div>
        </div>
      ))}

      {diagnostics.taskError && (
        <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-destructive">
          <XCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <div>
            <div className="font-medium">Scheduled Task status unavailable</div>
            <p className="break-words text-xs">{diagnostics.taskError}</p>
          </div>
        </div>
      )}

      {!diagnostics.taskInstalled && !diagnostics.taskError && (
        <div className="flex items-start gap-2 rounded-md border border-amber-300/70 bg-amber-50 px-3 py-2 text-amber-900 dark:border-amber-500/30 dark:bg-amber-950/40 dark:text-amber-200">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          <div>
            <div className="font-medium">Scheduled Task not installed</div>
            <p className="text-xs">{diagnostics.taskMessage}</p>
          </div>
        </div>
      )}

      {clean && (
        <div className="flex items-start gap-2 rounded-md border border-emerald-300/70 bg-emerald-50 px-3 py-2 text-emerald-800 dark:border-emerald-500/30 dark:bg-emerald-950/40 dark:text-emerald-200">
          <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0" />
          <div>
            <div className="font-medium">Ready</div>
            <p className="text-xs">
              All required tools are configured and the updater Scheduled Task is installed.
            </p>
          </div>
        </div>
      )}
    </div>
  );
}

const DOCTOR_CATEGORY_LABELS: Record<string, string> = {
  environment: "Environment",
  tools: "Tools",
  settings: "Settings",
  registry: "Registry",
  services: "Services",
  remote: "Remote access",
  task: "Scheduled Task",
  ssh: "SSH",
  storage: "Storage",
  pending: "Pending changes",
  php: "PHP references",
};

const DOCTOR_STATUS_LABELS: Record<DoctorStatus, string> = {
  ok: "OK",
  warning: "Warning",
  error: "Error",
  skipped: "Skipped",
};

function doctorStatusVariant(
  status: DoctorStatus,
): "success" | "warning" | "destructive" | "outline" {
  if (status === "ok") return "success";
  if (status === "warning") return "warning";
  if (status === "error") return "destructive";
  return "outline";
}

function doctorOverallLabel(report: DoctorReport | null, checking: boolean): string {
  if (checking) return "Checking";
  if (!report) return "Not run";
  if (report.overall === "ok") return "Healthy";
  if (report.overall === "warning") return "Warning";
  return "Error";
}

function doctorOverallVariant(
  report: DoctorReport | null,
  checking: boolean,
): "success" | "warning" | "destructive" | "outline" {
  if (checking || !report) return "outline";
  return doctorStatusVariant(report.overall);
}

function doctorCategoryLabel(category: string): string {
  return DOCTOR_CATEGORY_LABELS[category] ?? category;
}

function doctorReportText(report: DoctorReport): string {
  const lines: string[] = [
    "Service Manager Doctor",
    `Generated: ${report.generatedAt}`,
    `Overall: ${doctorOverallLabel(report, false)}`,
    `Base path: ${report.basePath}`,
    `Checks: OK ${report.counts.ok} | Warning ${report.counts.warning} | Error ${report.counts.error} | Skipped ${report.counts.skipped}`,
    "",
  ];
  let category = "";
  for (const check of report.checks) {
    if (check.category !== category) {
      category = check.category;
      lines.push(`[${doctorCategoryLabel(category).toUpperCase()}]`);
    }
    lines.push(
      `[${DOCTOR_STATUS_LABELS[check.status]}] ${check.label}: ${check.detail}`,
    );
    if (check.hint.trim()) lines.push(`  Hint: ${check.hint}`);
  }
  return lines.join("\n");
}

function DoctorChecks({ checks }: { checks: DoctorCheck[] }) {
  const grouped = new Map<string, DoctorCheck[]>();
  for (const check of checks) {
    const group = grouped.get(check.category);
    if (group) {
      group.push(check);
    } else {
      grouped.set(check.category, [check]);
    }
  }

  return (
    <div className="space-y-3">
      {Array.from(grouped.entries()).map(([category, categoryChecks]) => (
        <div key={category} className="space-y-1.5">
          <div className="flex items-center gap-2">
            <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              {doctorCategoryLabel(category)}
            </div>
            <Badge variant="outline">{categoryChecks.length}</Badge>
          </div>
          <div className="space-y-1.5">
            {categoryChecks.map((check) => (
              <div
                key={check.id}
                className="min-w-0 rounded-md border bg-background/40 px-3 py-2"
                title={`${check.id} · ${DOCTOR_STATUS_LABELS[check.status]}`}
              >
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div className="min-w-0 text-sm font-medium break-words">{check.label}</div>
                  <Badge variant={doctorStatusVariant(check.status)}>
                    {DOCTOR_STATUS_LABELS[check.status]}
                  </Badge>
                </div>
                <p className="mt-1 break-words text-xs text-muted-foreground">{check.detail}</p>
                {check.hint.trim() && (
                  <p className="mt-1 break-words text-xs text-muted-foreground">
                    <span className="font-medium">Hint:</span> {check.hint}
                  </p>
                )}
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function DoctorCard({
  report,
  loading,
  error,
  onRun,
  onCopy,
}: {
  report: DoctorReport | null;
  loading: boolean;
  error: string | null;
  onRun: () => void;
  onCopy: () => void;
}) {
  const overall = doctorOverallLabel(report, loading);
  return (
    <Card>
      <CardHeader className="p-4 pb-2">
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <CardTitle className="flex items-center gap-2 text-sm">
              <Stethoscope className="h-4 w-4" />
              Doctor
            </CardTitle>
            <CardDescription>
              Read-only readiness report for this server. Doctor does not save settings or change
              projects, services, or the pending Apache change.
            </CardDescription>
          </div>
          <Badge variant={doctorOverallVariant(report, loading)}>
            {loading && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
            {overall}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="space-y-3 p-4 pt-0">
        <div className="flex flex-wrap items-center gap-2">
          <Hint label="Run read-only readiness checks. No settings, services, projects, or Windows services are changed.">
            <Button type="button" size="sm" onClick={onRun} disabled={loading}>
              {loading ? <Loader2 className="animate-spin" /> : <Play />}
              Run Doctor
            </Button>
          </Hint>
          <Hint label="Copy the visible Doctor report as plain text to the clipboard">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={onCopy}
              disabled={loading || !report}
            >
              <ClipboardCopy />
              Copy Report
            </Button>
          </Hint>
          {report && (
            <div className="flex flex-wrap gap-1.5 text-xs">
              <Badge variant="success" title="Checks with status OK">
                OK {report.counts.ok}
              </Badge>
              <Badge variant="warning" title="Checks that need attention">
                Warning {report.counts.warning}
              </Badge>
              <Badge variant="destructive" title="Checks that block updates">
                Error {report.counts.error}
              </Badge>
              <Badge variant="outline" title="Checks that were not applicable or unavailable">
                Skipped {report.counts.skipped}
              </Badge>
            </div>
          )}
        </div>

        {report && (
          <div className="grid gap-1 rounded-md border bg-background/40 px-3 py-2 text-xs sm:grid-cols-2">
            <div>
              <div className="text-muted-foreground">Generated</div>
              <div className="break-words">{formatWhen(report.generatedAt)}</div>
            </div>
            <div className="min-w-0">
              <div className="text-muted-foreground">Base path</div>
              <div className="break-all font-mono">{report.basePath || "—"}</div>
            </div>
          </div>
        )}

        {error && (
          <div className="flex items-start gap-2 whitespace-pre-wrap break-words rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
            <XCircle className="mt-0.5 h-4 w-4 shrink-0" />
            <div>{error}</div>
          </div>
        )}

        {!report && !loading && !error && (
          <p className="text-sm text-muted-foreground">
            Click Run Doctor to check tools, settings, registry, services, remote access, Scheduled
            Task, SSH, storage, pending changes, and PHP references.
          </p>
        )}

        {report && !loading && <DoctorChecks checks={report.checks} />}
      </CardContent>
    </Card>
  );
}

function ApachePathChange({
  label,
  oldValue,
  newValue,
}: {
  label: string;
  oldValue: string;
  newValue: string;
}) {
  return (
    <div>
      <div className="text-muted-foreground">{label}</div>
      <div className="break-all font-mono text-xs">
        {oldValue || "(empty)"}
        <span className="px-1 text-muted-foreground">→</span>
        {newValue || "(empty)"}
      </div>
    </div>
  );
}

function ApacheChangePanel({
  pending,
  open,
  admin,
  applying,
  applyError,
  onReview,
  onApply,
  onLater,
}: {
  pending: ApacheChangeView;
  open: boolean;
  admin: boolean | null;
  applying: boolean;
  applyError: string | null;
  onReview: () => void;
  onApply: () => void;
  onLater: () => void;
}) {
  const canApply = admin === true && !applying;
  const applyHint =
    admin === true
      ? "Update services.json and apply the new Apache path to affected Windows services"
      : "Administrator privileges are required. The pending change stays saved until you apply it from an elevated Service Manager.";

  return (
    <Card className="border-amber-300/70 bg-amber-50/50 dark:border-amber-500/30 dark:bg-amber-950/20">
      <CardHeader className="p-4 pb-2">
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <CardTitle className="flex items-center gap-2 text-sm">
              <Server className="h-4 w-4" />
              Apache change pending
            </CardTitle>
            <CardDescription className="break-all">
              Saved {formatWhen(pending.createdAt)} · {pending.oldPath || "(not configured)"} →{" "}
              {pending.newPath}
            </CardDescription>
          </div>
          <Badge variant="warning" title="services.json has not been updated yet">
            Not applied · {pending.services.length} service
            {pending.services.length === 1 ? "" : "s"}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="space-y-3 p-4 pt-0">
        {!open ? (
          <div className="flex flex-wrap items-center justify-between gap-2">
            <p className="text-xs text-muted-foreground">
              Preview affected Apache services and apps before applying. Settings are already saved.
            </p>
            <div className="flex flex-wrap gap-2">
              <Button type="button" variant="outline" size="sm" onClick={onReview}>
                Review
              </Button>
              <Hint label={applyHint}>
                <Button
                  type="button"
                  size="sm"
                  onClick={onApply}
                  disabled={!canApply}
                >
                  {applying ? <Loader2 className="animate-spin" /> : <Play />}
                  Apply Now
                </Button>
              </Hint>
              <Button type="button" variant="ghost" size="sm" onClick={onLater}>
                Later
              </Button>
            </div>
          </div>
        ) : (
          <>
            <div className="grid gap-2 rounded-md border bg-background/60 p-3 text-xs sm:grid-cols-2">
              <ApachePathChange
                label="Apache executable"
                oldValue={pending.oldPath}
                newValue={pending.newPath}
              />
              <ApachePathChange
                label="Apache version"
                oldValue={pending.oldVersion}
                newValue={pending.newVersion}
              />
              <ApachePathChange
                label="Apache root"
                oldValue={pending.oldRoot}
                newValue={pending.newRoot}
              />
              <ApachePathChange
                label="Apache config"
                oldValue={pending.oldConfig}
                newValue={pending.newConfig}
              />
            </div>

            <div
              className={cn(
                "flex items-start gap-2 rounded-md border px-3 py-2 text-xs",
                admin === true
                  ? "border-border bg-muted/40 text-muted-foreground"
                  : "border-amber-300/70 bg-amber-50 text-amber-900 dark:border-amber-500/30 dark:bg-amber-950/40 dark:text-amber-200",
              )}
            >
              {admin === true ? (
                <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0" />
              ) : (
                <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" />
              )}
              <div>
                <div className="font-medium">
                  {admin === true
                    ? "Administrator detected"
                    : admin === false
                      ? "Administrator required"
                      : "Checking Administrator"}
                </div>
                <p>
                  Applying stops each affected Apache service, updates services.json, re-applies the
                  Windows service, then starts services that were running. services.json is backed up
                  automatically and restored if an apply step fails.
                </p>
              </div>
            </div>

            <div className="space-y-1">
              <div className="text-xs font-medium">Affected services</div>
              <div className="overflow-auto rounded-md border">
                <table className="w-full text-left text-xs">
                  <thead className="bg-muted/50">
                    <tr>
                      <th className="px-2 py-1.5 font-medium">Service</th>
                      <th className="px-2 py-1.5 font-medium">Executable</th>
                      <th className="px-2 py-1.5 font-medium">Root</th>
                    </tr>
                  </thead>
                  <tbody>
                    {pending.services.map((service) => (
                      <tr key={service.name} className="border-t">
                        <td className="px-2 py-1.5 font-medium">{service.name}</td>
                        <td className="break-all px-2 py-1.5 font-mono">
                          {service.oldExecutable}
                          <span className="px-1 text-muted-foreground">→</span>
                          {service.newExecutable}
                        </td>
                        <td className="break-all px-2 py-1.5 font-mono">
                          {service.oldRoot || "(empty)"}
                          <span className="px-1 text-muted-foreground">→</span>
                          {service.newRoot || "(empty)"}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>

            <div className="space-y-1">
              <div className="text-xs font-medium">Affected apps</div>
              {pending.apps.length === 0 ? (
                <p className="text-xs text-muted-foreground">
                  No app has webServerService pointing at an affected service.
                </p>
              ) : (
                <div className="flex flex-wrap gap-1">
                  {pending.apps.map((app) => (
                    <Badge
                      key={app.id}
                      variant="outline"
                      title={`webServerService links ${app.name} to an affected Apache service`}
                    >
                      {app.name || app.id}
                    </Badge>
                  ))}
                </div>
              )}
            </div>

            {applyError && (
              <div className="flex items-start gap-2 whitespace-pre-wrap break-words rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                <XCircle className="mt-0.5 h-4 w-4 shrink-0" />
                <div>{applyError}</div>
              </div>
            )}

            <div className="flex flex-wrap gap-2">
              <Hint label={applyHint}>
                <Button type="button" size="sm" onClick={onApply} disabled={!canApply}>
                  {applying ? <Loader2 className="animate-spin" /> : <Play />}
                  Apply Now
                </Button>
              </Hint>
              <Button type="button" variant="outline" size="sm" onClick={onLater}>
                Later
              </Button>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}

function ApacheApplyResultCard({
  result,
  onClose,
}: {
  result: ApacheApplyResult;
  onClose: () => void;
}) {
  const warning = result.success && result.errors.length > 0;
  return (
    <Card
      className={cn(
        "border-emerald-300/70 bg-emerald-50/50 dark:border-emerald-500/30 dark:bg-emerald-950/20",
        (!result.success || warning) &&
          "border-amber-300/70 bg-amber-50/50 dark:border-amber-500/30 dark:bg-amber-950/20",
      )}
    >
      <CardHeader className="p-4 pb-2">
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div>
            <CardTitle className="flex items-center gap-2 text-sm">
              {result.success ? (
                <CheckCircle2 className="h-4 w-4 text-emerald-600 dark:text-emerald-400" />
              ) : (
                <XCircle className="h-4 w-4 text-destructive" />
              )}
              {result.success ? "Apache apply complete" : "Apache apply failed"}
            </CardTitle>
            <CardDescription>
              Updated services: {result.services.join(", ") || "(none)"}
            </CardDescription>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={onClose}
            title="Hide this Apache apply result"
          >
            Close
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-2 p-4 pt-0 text-xs">
        {result.errors.length > 0 && (
          <div className="space-y-1 whitespace-pre-wrap break-words text-amber-900 dark:text-amber-200">
            {result.errors.map((error, index) => (
              <p key={`${index}-${error}`}>{error}</p>
            ))}
          </div>
        )}
        <div className="flex items-start gap-2">
          <Clock className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
          <div className="min-w-0">
            <div className="text-muted-foreground">Audit log</div>
            <div className="break-all font-mono">{result.auditPath || "—"}</div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

export function SettingsView({ admin }: { admin: boolean | null }) {
  const [view, setView] = useState<SettingsView | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [migratingTrigger, setMigratingTrigger] = useState(false);
  const [triggerIpsText, setTriggerIpsText] = useState("");
  const [taskStatus, setTaskStatus] = useState<TaskStatus | null>(null);
  const [taskStatusError, setTaskStatusError] = useState<string | null>(null);
  const [taskOperation, setTaskOperation] = useState<TaskOperation | null>(null);
  const [taskBusy, setTaskBusy] = useState<TaskAction | null>(null);
  const [apachePending, setApachePending] = useState<ApacheChangeView | null>(null);
  const [apachePreviewOpen, setApachePreviewOpen] = useState(false);
  const [apacheApplyResult, setApacheApplyResult] = useState<ApacheApplyResult | null>(null);
  const [apacheApplyError, setApacheApplyError] = useState<string | null>(null);
  const [applyingApache, setApplyingApache] = useState(false);
  const [apacheScan, setApacheScan] = useState<ApacheMappingScanView | null>(null);
  const [apacheDetecting, setApacheDetecting] = useState(false);
  const [apacheMappingApplying, setApacheMappingApplying] = useState(false);
  const [apacheMappingError, setApacheMappingError] = useState<string | null>(null);
  const [apacheSelectedService, setApacheSelectedService] = useState("");
  const [apacheSelectedApps, setApacheSelectedApps] = useState<Set<string>>(new Set());
  const [apacheServiceRows, setApacheServiceRows] = useState<ServiceRow[]>([]);
  const [phpPathChange, setPhpPathChange] = useState<PhpPathChangeView | null>(null);
  const [phpDialogOpen, setPhpDialogOpen] = useState(false);
  const [phpApplying, setPhpApplying] = useState<"variable" | "new_path" | null>(null);
  const [phpApplyError, setPhpApplyError] = useState<string | null>(null);
  const [doctor, setDoctor] = useState<DoctorReport | null>(null);
  const [doctorLoading, setDoctorLoading] = useState(false);
  const [doctorError, setDoctorError] = useState<string | null>(null);
  const [busyTools, setBusyTools] = useState<Partial<Record<ToolKey, ToolBusy>>>({});
  const [toolMessages, setToolMessages] = useState<
    Partial<Record<ToolKey, ToolMessage>>
  >({});
  const toolsBusy = Object.values(busyTools).some(
    (value) => value !== null && value !== undefined,
  );

  const refreshTaskStatus = useCallback(async (taskName: string) => {
    if (!taskName.trim()) {
      setTaskStatus(null);
      setTaskStatusError("Scheduled Task name is empty.");
      return;
    }
    try {
      const status = await invoke<TaskStatus>("task_status", { taskName });
      setTaskStatus(status);
      setTaskStatusError(status.error);
    } catch (error) {
      setTaskStatus(null);
      setTaskStatusError(errorMessage(error));
    }
  }, []);

  const loadApacheServices = useCallback(async () => {
    try {
      const rows = await invoke<ServiceRow[]>("list_services");
      const apacheRows = (Array.isArray(rows) ? rows : []).filter(
        (row) => row.serviceType === "apache",
      );
      setApacheServiceRows(apacheRows);
      setApacheSelectedService((previous) => {
        if (
          previous &&
          apacheRows.some((row) => row.name.toLowerCase() === previous.toLowerCase())
        ) {
          return previous;
        }
        return apacheRows.length === 1 ? apacheRows[0].name : "";
      });
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }, []);

  const load = useCallback(
    async (showToast: boolean) => {
      setLoading(true);
      try {
        const [next, pending] = await Promise.all([
          invoke<SettingsView>("load_settings_view"),
          invoke<ApacheChangeView | null>("apache_change_status"),
        ]);
        setView(next);
        setDraft(next.settings);
        setTriggerIpsText(next.settings.trigger.allowedClientIPs.join("\n"));
        setApachePending(pending);
        setApacheApplyError(null);
        setApacheApplyResult(null);
        setDoctor(null);
        setDoctorError(null);
        setApachePreviewOpen((previous) => (pending ? previous : false));
        await refreshTaskStatus(next.settings.privilegedExecution.taskName);
        if (showToast) toast.success("Settings reloaded.");
      } catch (error) {
        toast.error(errorMessage(error), { duration: 10000 });
      } finally {
        setLoading(false);
      }
    },
    [refreshTaskStatus],
  );

  useEffect(() => {
    void load(false);
  }, [load]);

  useEffect(() => {
    void loadApacheServices();
  }, [loadApacheServices]);

  function setToolBusy(key: ToolKey, busy: ToolBusy) {
    setBusyTools((previous) => ({ ...previous, [key]: busy }));
  }

  function setToolMessage(key: ToolKey, text: string, tone: ToolMessage["tone"]) {
    setToolMessages((previous) => ({ ...previous, [key]: { text, tone } }));
  }

  function updateTool(key: ToolKey, patch: Partial<ApacheToolPath>) {
    setDraft((previous) => {
      if (!previous) return previous;
      const tools = {
        ...previous.tools,
        [key]: { ...previous.tools[key], ...patch },
      } as SettingsTools;
      return { ...previous, tools };
    });
  }

  function changeToolPath(key: ToolKey, patch: Partial<ApacheToolPath>) {
    updateTool(key, patch);
    if (patch.path !== undefined) {
      setToolMessages((previous) => {
        if (!(key in previous)) return previous;
        const next = { ...previous };
        delete next[key];
        return next;
      });
    }
  }

  function updateUpdater(patch: Partial<Settings["updater"]>) {
    setDraft((previous) =>
      previous ? { ...previous, updater: { ...previous.updater, ...patch } } : previous,
    );
  }

  function updateTimeout(key: keyof UpdateTimeouts, value: number) {
    setDraft((previous) => {
      if (!previous) return previous;
      return {
        ...previous,
        updater: {
          ...previous.updater,
          timeouts: { ...previous.updater.timeouts, [key]: value },
        },
      };
    });
  }

  function updatePrivileged(patch: Partial<Settings["privilegedExecution"]>) {
    setDraft((previous) =>
      previous
        ? {
            ...previous,
            privilegedExecution: { ...previous.privilegedExecution, ...patch },
          }
        : previous,
    );
  }

  function updateTrigger(patch: Partial<SettingsTrigger>) {
    setDraft((previous) =>
      previous ? { ...previous, trigger: { ...previous.trigger, ...patch } } : previous,
    );
  }

  function changeTriggerIps(value: string) {
    setTriggerIpsText(value);
    updateTrigger({ allowedClientIPs: splitPatterns(value) });
  }

  async function browseTool(key: ToolKey) {
    const current = draft?.tools[key];
    if (!current) return;
    setToolBusy(key, "browse");
    try {
      const selected = await invoke<string | null>("pick_executable", {
        current: current.path || null,
      });
      if (!selected) return;
      if (key === "apache") {
        const inferred = inferApache(selected);
        updateTool(key, {
          path: selected,
          root: inferred.root,
          config: inferred.config,
          version: "",
          detectedAt: "",
        });
      } else {
        updateTool(key, { path: selected, version: "", detectedAt: "" });
      }
      setToolMessage(
        key,
        "Path selected. Run Auto-detect or Test to verify the version.",
        "info",
      );
    } catch (error) {
      toast.error(errorMessage(error));
    } finally {
      setToolBusy(key, null);
    }
  }

  async function detectTool(key: ToolKey) {
    const current = draft?.tools[key];
    if (!current) return;
    setToolBusy(key, "detect");
    setToolMessage(
      key,
      "Searching the configured path, PATH, and common install locations…",
      "info",
    );
    try {
      const result = await invoke<ToolDetection>("detect_tool", {
        tool: key,
        currentPath: current.path.trim() || null,
      });
      if (result.found) {
        const patch: Partial<ApacheToolPath> = {
          path: result.path,
          version: result.version,
          detectedAt: result.version ? new Date().toISOString() : "",
        };
        if (key === "apache") {
          patch.root = result.root;
          patch.config = result.config;
        }
        updateTool(key, patch);
      }
      setToolMessage(key, result.message, result.success ? "ok" : "error");
      if (result.success) {
        toast.success(`${toolLabel(key)} detected.`);
      } else {
        toast.error(result.message, { duration: 8000 });
      }
    } catch (error) {
      const message = errorMessage(error);
      setToolMessage(key, message, "error");
      toast.error(message, { duration: 8000 });
    } finally {
      setToolBusy(key, null);
    }
  }

  async function testTool(key: ToolKey) {
    const current = draft?.tools[key];
    if (!current) return;
    setToolBusy(key, "test");
    try {
      const result = await invoke<ToolTest>("test_tool", {
        tool: key,
        path: current.path.trim(),
      });
      if (result.success) {
        updateTool(key, {
          version: result.version,
          detectedAt: new Date().toISOString(),
        });
        setToolMessage(key, `Test passed: ${result.version || "(no version output)"}`, "ok");
        toast.success(`${toolLabel(key)} test passed.`);
      } else {
        const message = result.error || "Test failed.";
        setToolMessage(key, message, "error");
        toast.error(message, { duration: 8000 });
      }
    } catch (error) {
      const message = errorMessage(error);
      setToolMessage(key, message, "error");
      toast.error(message, { duration: 8000 });
    } finally {
      setToolBusy(key, null);
    }
  }

  async function browseLogDir() {
    if (!draft) return;
    try {
      const selected = await invoke<string | null>("pick_directory", {
        current: draft.updater.logDir || null,
      });
      if (selected) updateUpdater({ logDir: selected });
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }

  function toggleApacheApp(appId: string, checked: boolean) {
    setApacheSelectedApps((previous) => {
      const next = new Set(previous);
      if (checked) {
        next.add(appId);
      } else {
        next.delete(appId);
      }
      return next;
    });
  }

  function toggleAllApacheApps() {
    if (!apacheScan) return;
    const allSelected =
      apacheScan.suggestions.length > 0 &&
      apacheScan.suggestions.every((suggestion) =>
        apacheSelectedApps.has(suggestion.appId),
      );
    setApacheSelectedApps(
      allSelected
        ? new Set()
        : new Set(apacheScan.suggestions.map((suggestion) => suggestion.appId)),
    );
  }

  async function detectApacheMappings() {
    if (apacheDetecting) return;
    setApacheDetecting(true);
    setApacheMappingError(null);
    try {
      const result = await invoke<ApacheMappingScanView>("detect_apache_mappings", {
        serviceName: apacheSelectedService.trim() || null,
      });
      setApacheScan(result);
      if (result.serviceName) setApacheSelectedService(result.serviceName);
      setApacheSelectedApps(new Set(result.suggestions.map((item) => item.appId)));
      if (result.error) {
        toast.error(result.error, { duration: 10000 });
      } else if (result.suggestions.length === 0) {
        toast.warning("No new Apache app mappings were suggested.");
      } else {
        toast.success(
          `Found ${result.suggestions.length} Apache mapping suggestion${
            result.suggestions.length === 1 ? "" : "s"
          }.`,
        );
      }
    } catch (error) {
      const message = errorMessage(error);
      setApacheMappingError(message);
      toast.error(message, { duration: 10000 });
    } finally {
      setApacheDetecting(false);
    }
  }

  async function applyApacheMappings() {
    if (apacheMappingApplying) return;
    const serviceName = (apacheScan?.serviceName || apacheSelectedService).trim();
    const appIds = Array.from(apacheSelectedApps);
    if (!serviceName) {
      toast.error("Select an Apache service before applying mappings.");
      return;
    }
    if (appIds.length === 0) {
      toast.error("Select at least one suggested app to map.");
      return;
    }

    setApacheMappingApplying(true);
    setApacheMappingError(null);
    try {
      const views = await invoke<UpdaterApp[]>("apply_apache_mappings", {
        appIds,
        serviceName,
      });
      const wanted = new Set(appIds.map((id) => id.toLowerCase()));
      const updated = views.filter((view) => wanted.has(view.id.toLowerCase())).length;
      toast.success(
        `Mapped ${updated} app${updated === 1 ? "" : "s"} to ${serviceName}. projects.json was backed up first.`,
      );
      try {
        const result = await invoke<ApacheMappingScanView>("detect_apache_mappings", {
          serviceName,
        });
        setApacheScan(result);
        setApacheSelectedService(result.serviceName || serviceName);
        setApacheSelectedApps(new Set(result.suggestions.map((item) => item.appId)));
      } catch (refreshError) {
        toast.error(
          `Mappings were saved, but the rescan failed: ${errorMessage(refreshError)}`,
          { duration: 10000 },
        );
      }
    } catch (error) {
      const message = errorMessage(error);
      setApacheMappingError(message);
      toast.error(message, { duration: 10000 });
    } finally {
      setApacheMappingApplying(false);
    }
  }

  async function runDoctor() {
    setDoctorLoading(true);
    setDoctorError(null);
    try {
      const report = await invoke<DoctorReport>("run_doctor");
      setDoctor(report);
      if (report.overall === "ok") {
        toast.success("Doctor: server is ready.");
      } else if (report.overall === "warning") {
        toast.warning("Doctor finished with warnings.");
      } else {
        toast.error("Doctor found blocking errors.", { duration: 10000 });
      }
    } catch (error) {
      const message = errorMessage(error);
      setDoctorError(message);
      toast.error(message, { duration: 10000 });
    } finally {
      setDoctorLoading(false);
    }
  }

  async function copyDoctorReport() {
    if (!doctor) return;
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error("Clipboard is not available.");
      }
      await navigator.clipboard.writeText(doctorReportText(doctor));
      toast.success("Doctor report copied to the clipboard.");
    } catch (error) {
      toast.error(`Could not copy the Doctor report: ${errorMessage(error)}`, {
        duration: 10000,
      });
    }
  }

  async function save() {
    if (!draft || saving || toolsBusy) return;
    setSaving(true);
    try {
      const result = await invoke<SettingsSaveResult>("save_settings_view", {
        settings: draft,
      });
      const next = await invoke<SettingsView>("load_settings_view");
      setView(next);
      setDraft(next.settings);
      setTriggerIpsText(next.settings.trigger.allowedClientIPs.join("\n"));
      await refreshTaskStatus(next.settings.privilegedExecution.taskName);
      setApachePending(result.apachePending);
      setApachePreviewOpen(result.apachePending !== null);
      setApacheApplyResult(null);
      setApacheApplyError(null);
      setDoctor(null);
      setDoctorError(null);
      const reviewedPhpChange =
        result.phpPathChange && result.phpPathChange.references.length > 0
          ? result.phpPathChange
          : null;
      setPhpPathChange(reviewedPhpChange);
      setPhpDialogOpen(reviewedPhpChange !== null);
      setPhpApplyError(null);
      if (reviewedPhpChange) {
        toast.success(
          `Settings saved. ${reviewedPhpChange.references.length} hardcoded PHP reference${
            reviewedPhpChange.references.length === 1 ? "" : "s"
          } need review.`,
          { duration: 8000 },
        );
      } else if (result.apachePending) {
        toast.success("Settings saved. Review the pending Apache change before applying it.");
      } else {
        toast.success("Settings saved.");
      }
    } catch (error) {
      toast.error(errorMessage(error), { duration: 10000 });
    } finally {
      setSaving(false);
    }
  }

  function dismissPhpPathChange() {
    setPhpDialogOpen(false);
    setPhpPathChange(null);
    setPhpApplyError(null);
  }

  async function applyPhpReferences(mode: "variable" | "new_path") {
    if (!phpPathChange || phpApplying) return;
    const appIds = Array.from(
      new Set(phpPathChange.references.map((reference) => reference.appId)),
    );
    setPhpApplying(mode);
    setPhpApplyError(null);
    try {
      await invoke<UpdaterApp[]>("replace_php_references", {
        appIds,
        oldPath: phpPathChange.oldPath,
        oldRoot: phpPathChange.oldRoot,
        newPath: phpPathChange.newPath,
        newRoot: phpPathChange.newRoot,
        mode,
      });
      toast.success(
        mode === "variable"
          ? `Updated PHP references in ${appIds.length} app${
              appIds.length === 1 ? "" : "s"
            } to use \${PHP}/\${PHP_HOME}. projects.json was backed up first.`
          : `Updated PHP references in ${appIds.length} app${
              appIds.length === 1 ? "" : "s"
            } to the new PHP path. projects.json was backed up first.`,
        { duration: 8000 },
      );
      setDoctor(null);
      setDoctorError(null);
      dismissPhpPathChange();
    } catch (error) {
      const message = errorMessage(error);
      setPhpApplyError(message);
      toast.error(message, { duration: 10000 });
    } finally {
      setPhpApplying(null);
    }
  }

  async function applyApacheChange() {
    if (applyingApache) return;
    if (admin !== true) {
      toast.error(
        "Administrator privileges are required to apply the pending Apache change. Settings remain saved and the change stays pending.",
        { duration: 10000 },
      );
      return;
    }
    setApplyingApache(true);
    setApacheApplyError(null);
    setApacheApplyResult(null);
    try {
      const result = await invoke<ApacheApplyResult>("apply_apache_change");
      setApacheApplyResult(result);
      try {
        const pending = await invoke<ApacheChangeView | null>("apache_change_status");
        setApachePending(pending);
        setApachePreviewOpen(false);
        const next = await invoke<SettingsView>("load_settings_view");
        setView(next);
        setDraft(next.settings);
        setTriggerIpsText(next.settings.trigger.allowedClientIPs.join("\n"));
        await refreshTaskStatus(next.settings.privilegedExecution.taskName);
        setDoctor(null);
        setDoctorError(null);
      } catch (reloadError) {
        toast.error(`Apache apply finished, but reload failed: ${errorMessage(reloadError)}`, {
          duration: 10000,
        });
      }
      if (!result.success) {
        toast.error("Apache apply failed.", { duration: 10000 });
      } else if (result.errors.length > 0) {
        toast.warning("Apache services applied with warnings.", { duration: 10000 });
      } else {
        toast.success("Apache services applied.");
      }
    } catch (error) {
      const message = errorMessage(error);
      setApacheApplyError(message);
      try {
        const pending = await invoke<ApacheChangeView | null>("apache_change_status");
        setApachePending(pending);
        setApachePreviewOpen(pending !== null);
      } catch {
        // Keep the previous pending preview; the apply error is already shown.
      }
      toast.error(message, { duration: 10000 });
    } finally {
      setApplyingApache(false);
    }
  }

  async function migrateTrigger() {
    if (migratingTrigger) return;
    setMigratingTrigger(true);
    try {
      await invoke<Settings>("migrate_trigger_to_settings");
      const next = await invoke<SettingsView>("load_settings_view");
      setView(next);
      setDraft(next.settings);
      setTriggerIpsText(next.settings.trigger.allowedClientIPs.join("\n"));
      await refreshTaskStatus(next.settings.privilegedExecution.taskName);
      setDoctor(null);
      setDoctorError(null);
      toast.success("Trigger migrated from projects.json to settings.json.");
    } catch (error) {
      toast.error(errorMessage(error), { duration: 10000 });
    } finally {
      setMigratingTrigger(false);
    }
  }

  async function runTask(action: TaskAction) {
    if (!draft) return;
    if (action !== "test" && admin === false) {
      toast.error(
        "Administrator privileges are required. Restart Service Manager as Administrator.",
      );
      return;
    }
    setTaskBusy(action);
    setTaskOperation(null);
    try {
      let operation: TaskOperation;
      if (action === "install") {
        operation = await invoke<TaskOperation>("install_task", {
          taskName: draft.privilegedExecution.taskName,
          userId: draft.privilegedExecution.userId,
          logonType: draft.privilegedExecution.logonType,
        });
      } else if (action === "remove") {
        operation = await invoke<TaskOperation>("remove_task", {
          taskName: draft.privilegedExecution.taskName,
        });
      } else {
        operation = await invoke<TaskOperation>("test_task", {
          taskName: draft.privilegedExecution.taskName,
        });
      }
      setTaskOperation(operation);
      if (operation.success) {
        toast.success(
          action === "test" ? "Scheduled Task test finished." : "Scheduled Task updated.",
        );
      } else {
        toast.error(`Scheduled Task operation failed (exit ${operation.exitCode}).`, {
          duration: 10000,
        });
      }
      await refreshTaskStatus(draft.privilegedExecution.taskName);
      setDoctor(null);
      setDoctorError(null);
    } catch (error) {
      const message = errorMessage(error);
      setTaskStatusError(message);
      toast.error(message, { duration: 10000 });
    } finally {
      setTaskBusy(null);
    }
  }

  if (!draft) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center">
        <Card className="w-full max-w-lg">
          <CardHeader>
            <CardTitle className="text-base">Settings</CardTitle>
            <CardDescription>
              {loading
                ? "Loading settings and Scheduled Task status…"
                : "Settings could not be loaded. Check the toast message and reload."}
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Button
              type="button"
              variant="outline"
              onClick={() => void load(true)}
              disabled={loading}
            >
              {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              Reload
            </Button>
          </CardContent>
        </Card>
      </div>
    );
  }

  const diagnostics = view?.diagnostics;
  const task = taskBadge(taskStatus, taskStatusError);
  const dirty = view !== null && JSON.stringify(draft) !== JSON.stringify(view.settings);
  const legacyTrigger = view?.legacyTrigger ?? null;
  const triggerDiffers =
    view !== null && legacyTrigger !== null
      ? !sameTrigger(legacyTrigger, view.settings.trigger)
      : false;
  const canMigrateTrigger = view?.legacyFallback === true || triggerDiffers;
  const adminText =
    admin === true
      ? "Administrator"
      : admin === false
        ? "Not Administrator"
        : "Checking Administrator";
  const apacheServiceChoices =
    apacheServiceRows.length > 0
      ? apacheServiceRows.map((row) => ({
          name: row.name,
          label: row.displayName || row.name,
        }))
      : (apacheScan?.services ?? []).map((service) => ({
          name: service.name,
          label: service.displayName || service.name,
        }));
  const apacheMappingTarget = apacheScan?.serviceName || apacheSelectedService;
  const allApacheSuggestionsSelected =
    (apacheScan?.suggestions.length ?? 0) > 0 &&
    (apacheScan?.suggestions ?? []).every((suggestion) =>
      apacheSelectedApps.has(suggestion.appId),
    );
  const canApplyApacheMappings =
    apacheMappingTarget.trim().length > 0 &&
    apacheSelectedApps.size > 0 &&
    !apacheMappingApplying &&
    !apacheDetecting;
  const canDetectApacheMappings =
    apacheServiceChoices.length <= 1 || apacheSelectedService.trim().length > 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
      <div className="flex flex-wrap items-center gap-2 rounded-lg border bg-card px-3 py-2">
        <div className="mr-auto">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-sm font-semibold">Settings</h2>
            {dirty && <Badge variant="warning">Unsaved changes</Badge>}
          </div>
          <p className="text-xs text-muted-foreground">
            Toolchain paths, updater defaults, and privileged execution. Changes are written only
            when you click Save Settings.
          </p>
        </div>
        <Hint label="Reload settings and Scheduled Task status from disk">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void load(true)}
            disabled={loading || saving || taskBusy !== null || toolsBusy}
          >
            {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            Reload
          </Button>
        </Hint>
        <Hint label="Validate and save settings to data/settings.json">
          <Button
            type="button"
            size="sm"
            onClick={() => void save()}
            disabled={saving || loading || taskBusy !== null || toolsBusy}
          >
            {saving ? <Loader2 className="animate-spin" /> : <Save />}
            Save Settings
          </Button>
        </Hint>
      </div>

      <div className="min-h-0 flex-1 space-y-3 overflow-auto pr-1">
        {apachePending && (
          <ApacheChangePanel
            pending={apachePending}
            open={apachePreviewOpen}
            admin={admin}
            applying={applyingApache}
            applyError={apacheApplyError}
            onReview={() => setApachePreviewOpen(true)}
            onApply={() => void applyApacheChange()}
            onLater={() => setApachePreviewOpen(false)}
          />
        )}

        {apacheApplyResult && (
          <ApacheApplyResultCard
            result={apacheApplyResult}
            onClose={() => setApacheApplyResult(null)}
          />
        )}

        <Card>
          <CardHeader className="p-4 pb-2">
            <CardTitle className="flex items-center gap-2 text-sm">
              <Wrench className="h-4 w-4" />
              Toolchain
            </CardTitle>
            <CardDescription>
              Paths used by the updater. Auto-detect checks the configured path, PATH, and common
              install locations. Version metadata is only stored after a successful Test or
              Auto-detect.
            </CardDescription>
          </CardHeader>
          <CardContent className="p-4 pt-0">
            <div className="grid gap-3 xl:grid-cols-2 2xl:grid-cols-3">
              {TOOL_DEFINITIONS.map((definition) => (
                <ToolCard
                  key={definition.key}
                  definition={definition}
                  value={draft.tools[definition.key]}
                  busy={busyTools[definition.key] ?? null}
                  message={toolMessages[definition.key]}
                  onChange={(patch) => changeToolPath(definition.key, patch)}
                  onBrowse={() => void browseTool(definition.key)}
                  onDetect={() => void detectTool(definition.key)}
                  onTest={() => void testTool(definition.key)}
                />
              ))}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="p-4 pb-2">
            <CardTitle className="flex items-center gap-2 text-sm">
              <Server className="h-4 w-4" />
              Apache App Mapping
            </CardTitle>
            <CardDescription>
              Read the configured Apache config, includes, ServerRoot, and Define variables to
              suggest which apps each DocumentRoot serves. Nothing is written until you select
              apps and click Apply Mappings.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3 p-4 pt-0">
            <div className="flex flex-wrap items-end gap-2">
              {apacheServiceChoices.length > 1 ? (
                <div className="min-w-56 space-y-1">
                  <Label className="text-xs">Apache service</Label>
                  <Select
                    value={apacheSelectedService || undefined}
                    onValueChange={(value) => {
                      setApacheSelectedService(value);
                      setApacheScan(null);
                      setApacheSelectedApps(new Set());
                      setApacheMappingError(null);
                    }}
                  >
                    <SelectTrigger
                      className="h-8"
                      title="Choose which Apache service config to scan. Multiple Apache services use different ServerRoot/Include trees."
                    >
                      <SelectValue placeholder="Select Apache service" />
                    </SelectTrigger>
                    <SelectContent>
                      {apacheServiceChoices.map((service) => (
                        <SelectItem key={service.name} value={service.name}>
                          {service.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              ) : (
                <Badge
                  variant="outline"
                  title="Apache services come from data/services.json"
                >
                  {apacheServiceChoices.length === 1
                    ? `Service: ${apacheServiceChoices[0].name}`
                    : "No Apache service"}
                </Badge>
              )}
              <Hint label="Parse the selected Apache config, ServerRoot, Include/IncludeOptional files, and Define variables to suggest app mappings">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => void detectApacheMappings()}
                  disabled={
                    apacheDetecting ||
                    apacheMappingApplying ||
                    !canDetectApacheMappings
                  }
                >
                  {apacheDetecting ? <Loader2 className="animate-spin" /> : <Search />}
                  Detect Apps
                </Button>
              </Hint>
            </div>

            {(apacheMappingError || apacheScan?.error) && (
              <div className="flex items-start gap-2 whitespace-pre-wrap break-words rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                <XCircle className="mt-0.5 h-4 w-4 shrink-0" />
                <div>{apacheMappingError || apacheScan?.error}</div>
              </div>
            )}

            {apacheScan && apacheScan.warnings.length > 0 && (
              <div className="flex items-start gap-2 rounded-md border border-amber-300/70 bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:border-amber-500/30 dark:bg-amber-950/40 dark:text-amber-200">
                <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                <div>
                  <div className="font-medium">Parser warnings</div>
                  <ul className="mt-1 list-disc space-y-1 pl-4">
                    {apacheScan.warnings.map((warning, index) => (
                      <li key={`${index}-${warning}`} className="break-words">
                        {warning}
                      </li>
                    ))}
                  </ul>
                </div>
              </div>
            )}

            {!apacheScan && !apacheMappingError && (
              <p className="text-xs text-muted-foreground">
                Click Detect Apps to scan the selected Apache config for DocumentRoot entries and
                compare them with app localPath values.
              </p>
            )}

            {apacheScan !== null && apacheScan.suggestions.length === 0 && !apacheScan.error && (
              <p className="text-xs text-muted-foreground">
                No new app mappings are suggested. Existing mappings are shown below and are never
                overwritten automatically.
              </p>
            )}

            {apacheScan && apacheScan.suggestions.length > 0 && (
              <div className="space-y-2">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="text-xs font-medium">
                    Suggestions ({apacheScan.suggestions.length})
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={toggleAllApacheApps}
                    disabled={apacheMappingApplying}
                  >
                    {allApacheSuggestionsSelected ? "Clear all" : "Select all"}
                  </Button>
                </div>
                <div className="space-y-1">
                  {apacheScan.suggestions.map((suggestion) => {
                    const checkboxId = `apache-map-${suggestion.appId}`;
                    return (
                      <div
                        key={suggestion.appId}
                        className="flex items-start gap-3 rounded-md border bg-background/40 px-2 py-2"
                      >
                        <Checkbox
                          id={checkboxId}
                          checked={apacheSelectedApps.has(suggestion.appId)}
                          onCheckedChange={(checked) =>
                            toggleApacheApp(suggestion.appId, checked === true)
                          }
                          disabled={apacheMappingApplying}
                          className="mt-0.5"
                        />
                        <label
                          htmlFor={checkboxId}
                          className="min-w-0 flex-1 cursor-pointer text-xs leading-snug"
                        >
                          <span className="flex flex-wrap items-center gap-1.5">
                            <span className="text-sm font-medium">
                              {suggestion.appName || suggestion.appId}
                            </span>
                            <Badge variant="outline">{suggestion.appId}</Badge>
                            <Badge
                              variant={
                                suggestion.confidence === "high" ? "success" : "warning"
                              }
                              title={`${suggestion.matchKind} match`}
                            >
                              {suggestion.confidence} confidence
                            </Badge>
                            <Badge variant="outline" title="How localPath relates to DocumentRoot">
                              {suggestion.matchKind}
                            </Badge>
                          </span>
                          <span className="mt-1 block break-all font-mono text-muted-foreground">
                            {suggestion.localPath} → {suggestion.documentRoot}
                          </span>
                          <span className="block break-all text-muted-foreground">
                            Config: {suggestion.configFile || "—"}
                          </span>
                        </label>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {apacheScan && apacheScan.alreadyMapped.length > 0 && (
              <div className="space-y-2">
                <div className="text-xs font-medium">Already mapped</div>
                <div className="space-y-1">
                  {apacheScan.alreadyMapped.map((item) => (
                    <div
                      key={`mapped-${item.appId}`}
                      className="flex items-start gap-3 rounded-md border bg-muted/30 px-2 py-2 text-xs opacity-80"
                    >
                      <Checkbox
                        checked
                        disabled
                        className="mt-0.5"
                        title={`${item.appName || item.appId} is already mapped to ${item.existingService}`}
                      />
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-1.5">
                          <span className="text-sm font-medium">
                            {item.appName || item.appId}
                          </span>
                          <Badge variant="outline">{item.existingService}</Badge>
                          <Badge variant="success">Already mapped</Badge>
                        </div>
                        <div className="mt-1 break-all font-mono text-muted-foreground">
                          {item.localPath}
                          {item.documentRoot ? ` → ${item.documentRoot}` : ""}
                        </div>
                        <p className="text-muted-foreground">
                          Existing webServerService is kept; change it from the Updater app form if
                          needed.
                        </p>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            <div className="flex flex-wrap items-center justify-between gap-2 border-t pt-3">
              <div className="text-xs text-muted-foreground">
                {apacheMappingTarget
                  ? `Target service: ${apacheMappingTarget} · ${apacheSelectedApps.size} selected`
                  : "Select an Apache service first."}
              </div>
              <Hint label="Write webServerService for the selected apps to data/projects.json. projects.json is backed up to backup/projects-<timestamp>.json first; no other app fields change.">
                <Button
                  type="button"
                  size="sm"
                  onClick={() => void applyApacheMappings()}
                  disabled={!canApplyApacheMappings}
                >
                  {apacheMappingApplying ? <Loader2 className="animate-spin" /> : <Play />}
                  Apply Mappings
                </Button>
              </Hint>
            </div>
          </CardContent>
        </Card>

        <div className="grid gap-3 xl:grid-cols-2">
          <Card>
            <CardHeader className="p-4 pb-2">
              <CardTitle className="flex items-center gap-2 text-sm">
                <Terminal className="h-4 w-4" />
                Updater defaults
              </CardTitle>
              <CardDescription>
                Log location, concurrency, command timeouts, and log retention.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3 p-4 pt-0">
              <div className="space-y-1">
                <Label className="text-xs" htmlFor="settings-log-dir">
                  Log directory
                </Label>
                <div className="flex gap-2">
                  <Input
                    id="settings-log-dir"
                    value={draft.updater.logDir}
                    onChange={(event) => updateUpdater({ logDir: event.target.value })}
                    placeholder="C:\\tools\\ServiceManager\\logs"
                    className="h-8 flex-1 font-mono text-xs"
                    title="Updater logs are written here. Empty uses the default logs folder next to the config."
                  />
                  <Hint label="Browse for the updater log directory">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => void browseLogDir()}
                    >
                      <FolderOpen />
                      Browse…
                    </Button>
                  </Hint>
                </div>
              </div>

              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-1">
                  <Label className="text-xs" htmlFor="settings-max-concurrent">
                    Max concurrent updates
                  </Label>
                  <Input
                    id="settings-max-concurrent"
                    type="number"
                    min={1}
                    max={32}
                    value={draft.updater.maxConcurrentUpdates}
                    onChange={(event) =>
                      updateUpdater({
                        maxConcurrentUpdates: Number.parseInt(event.target.value, 10) || 0,
                      })
                    }
                    className="h-8"
                    title="Number of apps updated at the same time (1–32)."
                  />
                </div>
                <div className="space-y-1">
                  <Label className="text-xs" htmlFor="settings-retention">
                    Log retention days
                  </Label>
                  <Input
                    id="settings-retention"
                    type="number"
                    min={0}
                    max={3650}
                    value={draft.updater.logRetentionDays}
                    onChange={(event) =>
                      updateUpdater({
                        logRetentionDays: Number.parseInt(event.target.value, 10) || 0,
                      })
                    }
                    className="h-8"
                    title="Delete updater logs older than this many days (0–3650)."
                  />
                </div>
              </div>

              <div className="grid gap-3 sm:grid-cols-2">
                {TIMEOUT_FIELDS.map((field) => (
                  <div className="space-y-1" key={field.key}>
                    <Label className="text-xs" htmlFor={`settings-${field.key}`} title={field.hint}>
                      {field.label}
                    </Label>
                    <Input
                      id={`settings-${field.key}`}
                      type="number"
                      min={1}
                      max={86400}
                      value={draft.updater.timeouts[field.key]}
                      onChange={(event) =>
                        updateTimeout(
                          field.key,
                          Number.parseInt(event.target.value, 10) || 0,
                        )
                      }
                      className="h-8"
                      title={`${field.hint} Range: 1–86400 seconds.`}
                    />
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="p-4 pb-2">
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div>
                  <CardTitle className="flex items-center gap-2 text-sm">
                    <ShieldCheck className="h-4 w-4" />
                    Privileged Execution
                  </CardTitle>
                  <CardDescription>
                    Elevated updater trigger as a Windows Scheduled Task. No Windows Service is
                    created.
                  </CardDescription>
                </div>
                <Badge variant={task.variant}>{task.label}</Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-3 p-4 pt-0">
              <div
                className={cn(
                  "flex items-start gap-2 rounded-md border px-3 py-2 text-xs",
                  admin === false
                    ? "border-amber-300/70 bg-amber-50 text-amber-900 dark:border-amber-500/30 dark:bg-amber-950/40 dark:text-amber-200"
                    : "border-border bg-muted/40 text-muted-foreground",
                )}
              >
                {admin === false ? (
                  <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" />
                ) : (
                  <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0" />
                )}
                <div>
                  <div className="font-medium">{adminText}</div>
                  <p>
                    Install, Repair, and Remove require Administrator privileges. Test Task can run
                    without elevation but the task itself must already exist.
                  </p>
                </div>
              </div>

              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-1">
                  <Label className="text-xs" htmlFor="settings-task-name">
                    Task name
                  </Label>
                  <Input
                    id="settings-task-name"
                    value={draft.privilegedExecution.taskName}
                    onChange={(event) => updatePrivileged({ taskName: event.target.value })}
                    className="h-8"
                    title="Windows Scheduled Task name. Letters, numbers, spaces, dots, underscores, and hyphens only."
                  />
                </div>
                <div className="space-y-1">
                  <Label className="text-xs" htmlFor="settings-user-id">
                    User
                  </Label>
                  <Input
                    id="settings-user-id"
                    value={draft.privilegedExecution.userId}
                    onChange={(event) => updatePrivileged({ userId: event.target.value })}
                    placeholder="DOMAIN\\user or empty for current user"
                    className="h-8"
                    title="Account that owns the Scheduled Task. Leave empty to use the current user at install time."
                  />
                </div>
                <div className="space-y-1">
                  <Label className="text-xs">Logon type</Label>
                  <Select
                    value={draft.privilegedExecution.logonType}
                    onValueChange={(value) => updatePrivileged({ logonType: value })}
                  >
                    <SelectTrigger
                      className="h-8"
                      title="Interactive stores no password. S4U runs without a stored password."
                    >
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="Interactive">Interactive</SelectItem>
                      <SelectItem value="S4U">S4U</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-1">
                  <Label className="text-xs" htmlFor="settings-task-state">
                    State
                  </Label>
                  <Input
                    id="settings-task-state"
                    readOnly
                    value={taskStatus?.state || (taskStatusError ? "Unavailable" : "Unknown")}
                    className="h-8"
                    title="Current Windows Scheduled Task state."
                  />
                </div>
              </div>

              <div className="grid gap-2 rounded-md border bg-background/40 p-3 text-xs sm:grid-cols-2">
                <div>
                  <div className="text-muted-foreground">Run level</div>
                  <div>{taskStatus?.runLevel || "—"}</div>
                </div>
                <div>
                  <div className="text-muted-foreground">User</div>
                  <div className="break-all">{taskStatus?.user || "—"}</div>
                </div>
                <div>
                  <div className="text-muted-foreground">Logon type</div>
                  <div>{taskStatus?.logonType || "—"}</div>
                </div>
                <div>
                  <div className="text-muted-foreground">Last run</div>
                  <div>{formatWhen(taskStatus?.lastRun ?? "")}</div>
                </div>
                <div>
                  <div className="text-muted-foreground">Last result</div>
                  <div>
                    {taskStatus
                      ? taskStatus.lastResult === 0
                        ? "0 (success)"
                        : String(taskStatus.lastResult)
                      : "—"}
                  </div>
                </div>
                <div>
                  <div className="text-muted-foreground">Task name</div>
                  <div className="break-all">{taskStatus?.taskName || "—"}</div>
                </div>
              </div>

              {taskStatusError && (
                <p className="break-words text-xs text-destructive">{taskStatusError}</p>
              )}

              <div className="flex flex-wrap gap-2">
                <Hint label="Register or repair the Scheduled Task with the configured name, user, and logon type">
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => void runTask("install")}
                    disabled={taskBusy !== null || admin === false}
                  >
                    {taskBusy === "install" ? <Loader2 className="animate-spin" /> : <Wrench />}
                    {taskStatus?.installed ? "Repair Task" : "Install Task"}
                  </Button>
                </Hint>
                <Hint label="Unregister the Scheduled Task (Administrator required)">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void runTask("remove")}
                    disabled={
                      taskBusy !== null ||
                      admin === false ||
                      (taskStatus !== null && !taskStatus.installed)
                    }
                  >
                    {taskBusy === "remove" ? <Loader2 className="animate-spin" /> : <Trash2 />}
                    Remove Task
                  </Button>
                </Hint>
                <Hint label="Run scripts/Invoke-UpdaterTask.ps1 -Check for the saved task name and show the output">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void runTask("test")}
                    disabled={taskBusy !== null}
                  >
                    {taskBusy === "test" ? <Loader2 className="animate-spin" /> : <FlaskConical />}
                    Test Task
                  </Button>
                </Hint>
              </div>

              {taskOperation && (
                <div className="space-y-1">
                  <div className="flex items-center justify-between gap-2">
                    <div className="text-xs font-medium">
                      Task output (exit {taskOperation.exitCode})
                    </div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => setTaskOperation(null)}
                      title="Hide the last Scheduled Task output"
                    >
                      Close
                    </Button>
                  </div>
                  <pre
                    className={cn(
                      "max-h-56 overflow-auto whitespace-pre-wrap break-words rounded-md border p-2 font-mono text-xs",
                      taskOperation.success ? "" : "text-destructive",
                    )}
                  >
                    {taskOperation.output}
                  </pre>
                </div>
              )}
            </CardContent>
          </Card>
        </div>

        <Card>
          <CardHeader className="p-4 pb-2">
            <div className="flex flex-wrap items-start justify-between gap-2">
              <div>
                <CardTitle className="flex items-center gap-2 text-sm">
                  <Network className="h-4 w-4" />
                  Trigger
                </CardTitle>
                <CardDescription>
                  Global client IP allowlist and local trigger policy. settings.json wins over
                  projects.json when the file exists.
                </CardDescription>
              </div>
              <Badge variant="outline" title="Detected from SSH_CONNECTION/SSH_CLIENT">
                {view?.currentClientIp ? `Client IP: ${view.currentClientIp}` : "Client IP: not detected"}
              </Badge>
            </div>
          </CardHeader>
          <CardContent className="space-y-3 p-4 pt-0">
            <div className="space-y-1">
              <Label className="text-xs" htmlFor="settings-trigger-ips">
                Allowed IPs / CIDR
              </Label>
              <textarea
                id="settings-trigger-ips"
                value={triggerIpsText}
                onChange={(event) => changeTriggerIps(event.target.value)}
                rows={3}
                placeholder={"10.255.240.143, 10.255.240.0/24\n127.0.0.1, ::1"}
                className="w-full rounded-md border border-input bg-background px-3 py-2 font-mono text-xs ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
                title="Comma or newline separated. Accepts IPv4, IPv6, and CIDR. Empty list allows any remote IP."
              />
              <p className="text-xs text-muted-foreground">
                Comma or newline separated. Accepts IPv4, IPv6, and CIDR. An empty list allows
                any remote IP; use Allow local to control non-SSH triggers.
              </p>
            </div>

            <div className="flex items-center justify-between gap-4 rounded-md border px-3 py-2">
              <div>
                <Label htmlFor="settings-trigger-local">Allow local trigger</Label>
                <p className="text-xs text-muted-foreground">
                  Permit runs without an SSH client IP (local terminal or GUI).
                </p>
              </div>
              <Switch
                id="settings-trigger-local"
                checked={draft.trigger.allowLocal}
                onCheckedChange={(checked) => updateTrigger({ allowLocal: checked })}
              />
            </div>

            <div className="rounded-md border bg-background/40 px-3 py-2 text-xs">
              <div className="text-muted-foreground">Current client IP</div>
              <div className="font-mono">{view?.currentClientIp || "not detected"}</div>
              <p className="mt-1 text-muted-foreground">
                {view?.legacyFallback
                  ? "settings.json is absent; the effective trigger currently comes from projects.json."
                  : triggerDiffers
                    ? "projects.json still has a different trigger. Migrate only when you want settings.json to replace it."
                    : "settings.json is the active trigger source."}
              </p>
            </div>

            {canMigrateTrigger && (
              <div className="flex flex-wrap items-start justify-between gap-2 rounded-md border border-amber-300/70 bg-amber-50 px-3 py-2 text-amber-900 dark:border-amber-500/30 dark:bg-amber-950/40 dark:text-amber-200">
                <div className="min-w-0">
                  <div className="text-sm font-medium">Legacy trigger in projects.json</div>
                  <p className="text-xs">
                    Copy allowedClientIPs and allowLocal into settings.json. projects.json is not
                    modified. Unsaved edits in this form are discarded by the migration.
                  </p>
                </div>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => void migrateTrigger()}
                  disabled={migratingTrigger || saving || toolsBusy}
                  title="Copy projects.json trigger into data/settings.json"
                >
                  {migratingTrigger ? <Loader2 className="animate-spin" /> : <ArrowRightLeft />}
                  Migrate Trigger
                </Button>
              </div>
            )}
          </CardContent>
        </Card>

        <DoctorCard
          report={doctor}
          loading={doctorLoading}
          error={doctorError}
          onRun={() => void runDoctor()}
          onCopy={() => void copyDoctorReport()}
        />

        <Card>
          <CardHeader className="p-4 pb-2">
            <CardTitle className="flex items-center gap-2 text-sm">
              <AlertTriangle className="h-4 w-4" />
              Diagnostics
            </CardTitle>
            <CardDescription>
              Readiness summary for the saved configuration: required tools, path validity,
              Scheduled Task state, and legacy fallback.
            </CardDescription>
          </CardHeader>
          <CardContent className="p-4 pt-0">
            <Diagnostics diagnostics={diagnostics} />
          </CardContent>
        </Card>
      </div>

      <Dialog
        open={phpDialogOpen && phpPathChange !== null}
        onOpenChange={(open) => {
          if (open) {
            setPhpDialogOpen(true);
          } else {
            dismissPhpPathChange();
          }
        }}
      >
        <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-3xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Terminal className="h-4 w-4" />
              PHP references found
            </DialogTitle>
            <DialogDescription>
              The saved PHP path changed, but some app commands still contain the old PHP path or
              root. Nothing is rewritten until you choose an action.
            </DialogDescription>
          </DialogHeader>

          {phpPathChange && (
            <div className="space-y-3">
              <div className="grid gap-2 rounded-md border bg-background/40 p-3 text-xs sm:grid-cols-2">
                <div className="min-w-0">
                  <div className="text-muted-foreground">Old PHP path</div>
                  <div className="break-all font-mono">{phpPathChange.oldPath || "—"}</div>
                  <div className="mt-1 break-all font-mono text-muted-foreground">
                    Old PHP root: {phpPathChange.oldRoot || "—"}
                  </div>
                </div>
                <div className="min-w-0">
                  <div className="text-muted-foreground">New PHP path</div>
                  <div className="break-all font-mono">{phpPathChange.newPath || "—"}</div>
                  <div className="mt-1 break-all font-mono text-muted-foreground">
                    New PHP root: {phpPathChange.newRoot || "—"}
                  </div>
                </div>
              </div>

              <div className="space-y-1">
                <div className="text-xs font-medium">
                  References ({phpPathChange.references.length})
                </div>
                <div className="max-h-64 space-y-1 overflow-auto pr-1">
                  {phpPathChange.references.map((reference, index) => (
                    <div
                      key={`${reference.appId}-${reference.field}-${index}`}
                      className="rounded-md border bg-background/40 px-2 py-2 text-xs"
                    >
                      <div className="flex flex-wrap items-center gap-1.5">
                        <span className="font-medium">
                          {reference.appName || reference.appId}
                        </span>
                        <Badge variant="outline" title="App id">
                          {reference.appId}
                        </Badge>
                        <Badge
                          variant="secondary"
                          title="Command field that contains the old PHP path/root"
                        >
                          {reference.field}
                        </Badge>
                      </div>
                      <pre className="mt-1 whitespace-pre-wrap break-all font-mono text-muted-foreground">
                        {reference.value}
                      </pre>
                    </div>
                  ))}
                </div>
              </div>

              <p className="text-xs text-muted-foreground">
                <strong>Use {"${PHP}"}</strong> rewrites the old PHP executable to {"${PHP}"} and the
                old PHP folder to {"${PHP_HOME}"}. <strong>Use new path</strong> writes the saved new
                path/root literally. Apply only to apps listed above; other apps are not changed.
                projects.json is backed up first.
              </p>

              {phpApplyError && (
                <div className="flex items-start gap-2 whitespace-pre-wrap break-words rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                  <XCircle className="mt-0.5 h-4 w-4 shrink-0" />
                  <div>{phpApplyError}</div>
                </div>
              )}
            </div>
          )}

          <DialogFooter className="gap-2">
            <Button
              type="button"
              onClick={() => void applyPhpReferences("variable")}
              disabled={phpApplying !== null}
              title="Replace the old PHP path with ${PHP} and the old root with ${PHP_HOME}"
            >
              {phpApplying === "variable" ? <Loader2 className="animate-spin" /> : null}
              {"Use ${PHP}"}
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={() => void applyPhpReferences("new_path")}
              disabled={phpApplying !== null || !phpPathChange?.newPath.trim()}
              title="Replace the old PHP path/root with the saved new path/root in the listed apps"
            >
              {phpApplying === "new_path" ? <Loader2 className="animate-spin" /> : null}
              Use new path
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={() => dismissPhpPathChange()}
              disabled={phpApplying !== null}
              title="Close this dialog without changing projects.json"
            >
              Leave
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
