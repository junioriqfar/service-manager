import { useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  FlaskConical,
  FolderOpen,
  Loader2,
  RefreshCw,
  Save,
  Search,
  ShieldAlert,
  ShieldCheck,
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { errorMessage, invoke } from "@/lib/tauri";
import type {
  ApacheToolPath,
  Settings,
  SettingsDiagnostics,
  SettingsTools,
  SettingsView,
  TaskOperation,
  TaskStatus,
  ToolDetection,
  ToolPath,
  ToolTest,
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
    description: "PHP CLI used by Laravel commands and Composer.",
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

export function SettingsView({ admin }: { admin: boolean | null }) {
  const [view, setView] = useState<SettingsView | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [taskStatus, setTaskStatus] = useState<TaskStatus | null>(null);
  const [taskStatusError, setTaskStatusError] = useState<string | null>(null);
  const [taskOperation, setTaskOperation] = useState<TaskOperation | null>(null);
  const [taskBusy, setTaskBusy] = useState<TaskAction | null>(null);
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

  const load = useCallback(
    async (showToast: boolean) => {
      setLoading(true);
      try {
        const next = await invoke<SettingsView>("load_settings_view");
        setView(next);
        setDraft(next.settings);
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

  async function save() {
    if (!draft || saving || toolsBusy) return;
    setSaving(true);
    try {
      await invoke<Settings>("save_settings_view", { settings: draft });
      const next = await invoke<SettingsView>("load_settings_view");
      setView(next);
      setDraft(next.settings);
      await refreshTaskStatus(next.settings.privilegedExecution.taskName);
      toast.success("Settings saved.");
    } catch (error) {
      toast.error(errorMessage(error), { duration: 10000 });
    } finally {
      setSaving(false);
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
  const adminText =
    admin === true
      ? "Administrator"
      : admin === false
        ? "Not Administrator"
        : "Checking Administrator";

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
    </div>
  );
}
