import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  AlertTriangle,
  ClipboardList,
  FileText,
  FlaskConical,
  History,
  ListChecks,
  Loader2,
  MoreHorizontal,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";

import { ConfirmDialog } from "@/components/confirm-dialog";
import { Hint } from "@/components/hint";
import { ServiceStatusBadge, UpdaterStatusBadge } from "@/components/status-badge";
import { TooltipText } from "@/components/tooltip-text";
import { UpdaterFormDialog } from "@/components/updater-form-dialog";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ModalShell } from "@/components/ui/modal-shell";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { errorMessage, invoke } from "@/lib/tauri";
import type {
  AppPlan,
  MaintenanceView,
  PlanCommandGroup,
  PlanService,
  UpdaterApp,
  UpdaterHistoryEntry,
  UpdaterLogView,
  UpdaterRunOutcome,
} from "@/lib/types";

function formatWhen(value: string): string {
  if (!value) return "—";
  return value.replace("T", " ").slice(0, 19);
}

function serviceSummary(app: UpdaterApp): string {
  const names = [
    ...app.services.stopBeforePull,
    ...app.services.stopBeforeBuild,
    ...app.services.restartAfter,
    ...app.services.startAfter,
  ];
  return names.length ? Array.from(new Set(names)).join(", ") : "—";
}

function shortSha(value: string): string {
  return value ? value.slice(0, 12) : "—";
}

function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "0 ms";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(1)} s`;
}

function PlanSection({
  title,
  count,
  children,
}: {
  title: string;
  count?: number;
  children: ReactNode;
}) {
  return (
    <section className="space-y-1.5">
      <h4 className="text-sm font-medium">
        {title}
        {count !== undefined && (
          <span className="ml-1 font-normal text-muted-foreground">({count})</span>
        )}
      </h4>
      {children}
    </section>
  );
}

function PlanServiceRow({ service }: { service: PlanService }) {
  const running = service.running;
  return (
    <li className="px-3 py-1.5 text-xs">
      <div className="flex items-center justify-between gap-2">
        <span className="font-medium">{service.name}</span>
        <ServiceStatusBadge
          state={running === true ? "running" : running === false ? "stopped" : "unknown"}
          label={running === true ? "running" : running === false ? "stopped" : "unknown"}
          hint={
            running === true
              ? "This service will be handled by the plan and is currently running."
              : running === false
                ? "This service is currently stopped; the plan starts it only when configured to."
                : "The plan could not read the current service state; validation is limited."
          }
        />
      </div>
      {service.error && <div className="mt-1 text-destructive">{service.error}</div>}
    </li>
  );
}

function PlanDetails({ plan }: { plan: AppPlan }) {
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <UpdaterStatusBadge status={plan.status} />
        <span className="text-sm text-muted-foreground">{plan.message}</span>
      </div>

      <div className="grid gap-2 rounded-md border px-3 py-2 text-sm sm:grid-cols-2">
        <div>
          <span className="text-muted-foreground">Branch: </span>
          {plan.branch}
          {plan.currentBranch ? ` (active ${plan.currentBranch})` : ""}
        </div>
        <div>
          <span className="text-muted-foreground">Incoming: </span>
          {plan.incoming.length} commit(s)
        </div>
        <div>
          <span className="text-muted-foreground">HEAD: </span>
          <span className="font-mono text-xs">{shortSha(plan.headBefore)}</span>
        </div>
        <div>
          <span className="text-muted-foreground">Remote HEAD: </span>
          <span className="font-mono text-xs">{shortSha(plan.remoteHead)}</span>
        </div>
      </div>

      {plan.warnings.length > 0 && (
        <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm">
          <div className="font-medium">Warnings</div>
          <ul className="mt-1 max-h-40 list-disc space-y-1 overflow-auto pl-5 text-xs text-muted-foreground">
            {plan.warnings.map((warning, index) => (
              <li key={`${index}-${warning}`}>{warning}</li>
            ))}
          </ul>
        </div>
      )}

      <PlanSection title="Incoming commits" count={plan.incoming.length}>
        {plan.incoming.length === 0 ? (
          <p className="text-sm text-muted-foreground">No incoming commits.</p>
        ) : (
          <div className="max-h-40 overflow-auto rounded-md border">
            <ul className="divide-y text-sm">
              {plan.incoming.map((commit) => (
                <li key={commit.sha} className="flex gap-2 px-3 py-1.5">
                  <span className="shrink-0 font-mono text-xs text-muted-foreground">
                    {shortSha(commit.sha)}
                  </span>
                  <span className="min-w-0 break-words">
                    {commit.subject || "(no subject)"}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </PlanSection>

      <PlanSection title="Changed files" count={plan.changedFiles.length}>
        {plan.changedFiles.length === 0 ? (
          <p className="text-sm text-muted-foreground">No changed files.</p>
        ) : (
          <div className="max-h-40 overflow-auto rounded-md border p-2 font-mono text-xs">
            {plan.changedFiles.map((file, index) => (
              <div key={`${index}-${file}`} className="break-all">
                {file}
              </div>
            ))}
          </div>
        )}
      </PlanSection>

      <PlanSection title="Migration blockers" count={plan.migrationFiles.length}>
        {plan.migrationFiles.length === 0 ? (
          <p className="text-sm text-muted-foreground">No migration changes.</p>
        ) : (
          <div className="max-h-40 overflow-auto rounded-md border border-amber-500/40 bg-amber-500/10 p-2 font-mono text-xs">
            {plan.migrationFiles.map((file, index) => (
              <div key={`${index}-${file}`} className="break-all">
                {file}
              </div>
            ))}
          </div>
        )}
      </PlanSection>

      <PlanSection title="Commands" count={plan.commands.length}>
        {plan.commands.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No toolchain commands previewed.
          </p>
        ) : (
          <div className="space-y-3">
            {(["install", "build", "artisan"] as PlanCommandGroup[]).map((group) => {
              const commands = plan.commands.filter((command) => command.group === group);
              if (commands.length === 0) return null;
              return (
                <div key={group} className="space-y-1">
                  <div className="text-xs font-medium uppercase text-muted-foreground">
                    {group}
                  </div>
                  <div className="max-h-40 space-y-2 overflow-auto rounded-md border p-2">
                    {commands.map((command, index) => (
                      <div key={`${group}-${index}-${command.label}`} className="text-xs">
                        <div className="font-medium">{command.label}</div>
                        <div className="break-all font-mono text-muted-foreground">
                          {command.description}
                        </div>
                        <div className="break-all font-mono text-muted-foreground">
                          cwd: {command.cwd}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </PlanSection>

      <PlanSection title="Services (read-only validation)">
        <div className="space-y-1.5 text-sm">
          <div>
            <span className="text-muted-foreground">Validation: </span>
            {plan.services.validation || "unknown"}
          </div>
          {plan.services.stopBeforePull.length > 0 && (
            <div>
              <span className="text-muted-foreground">Stop before pull: </span>
              {plan.services.stopBeforePull.join(", ")}
            </div>
          )}
          {plan.services.stopBeforeBuild.length > 0 && (
            <div>
              <span className="text-muted-foreground">Stop before build: </span>
              {plan.services.stopBeforeBuild.join(", ")}
            </div>
          )}
          {plan.services.restartAfter.length > 0 && (
            <div>
              <span className="text-muted-foreground">Restart after: </span>
              {plan.services.restartAfter.join(", ")}
            </div>
          )}
          {plan.services.startAfter.length > 0 && (
            <div>
              <span className="text-muted-foreground">Start after: </span>
              {plan.services.startAfter.join(", ")}
            </div>
          )}
          {plan.services.states.length === 0 ? (
            <p className="text-muted-foreground">No service lifecycle configured.</p>
          ) : (
            <div className="max-h-40 overflow-auto rounded-md border">
              <ul className="divide-y">
                {plan.services.states.map((service) => (
                  <PlanServiceRow key={service.name} service={service} />
                ))}
              </ul>
            </div>
          )}
        </div>
      </PlanSection>

      <PlanSection title="Health check and webhook">
        <div className="space-y-1 text-sm">
          <div>
            <span className="text-muted-foreground">Health check: </span>
            {plan.healthCheck.enabled
              ? `${plan.healthCheck.url} (${plan.healthCheck.timeoutSeconds}s)`
              : "disabled"}
          </div>
          <div>
            <span className="text-muted-foreground">Webhook: </span>
            {plan.notify.enabled ? plan.notify.webhookUrl : "disabled"}
            {plan.notify.on.length > 0 ? ` (on: ${plan.notify.on.join(", ")})` : ""}
          </div>
        </div>
      </PlanSection>
    </div>
  );
}

export function UpdaterView() {
  const [apps, setApps] = useState<UpdaterApp[]>([]);
  const [maintenance, setMaintenance] = useState<MaintenanceView | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [running, setRunning] = useState<"check" | "update" | null>(null);
  const [logView, setLogView] = useState<UpdaterLogView | null>(null);
  const [logLoading, setLogLoading] = useState(false);
  const [outcomes, setOutcomes] = useState<UpdaterRunOutcome[] | null>(null);
  const [testDialog, setTestDialog] = useState<{
    open: boolean;
    title: string;
    text: string;
    error: boolean;
  }>({ open: false, title: "", text: "", error: false });
  const [form, setForm] = useState<{ open: boolean; edit: boolean; app: UpdaterApp | null }>({
    open: false,
    edit: false,
    app: null,
  });
  const [deleteTarget, setDeleteTarget] = useState<UpdaterApp | null>(null);
  const [planDialog, setPlanDialog] = useState<{
    open: boolean;
    loading: boolean;
    appId: string;
    plan: AppPlan | null;
    error: string;
  }>({ open: false, loading: false, appId: "", plan: null, error: "" });
  const [historyDialog, setHistoryDialog] = useState<{
    open: boolean;
    loading: boolean;
    appId: string;
    entries: UpdaterHistoryEntry[];
    error: string;
  }>({ open: false, loading: false, appId: "", entries: [], error: "" });
  const planRequest = useRef(0);
  const historyRequest = useRef(0);

  const selected = apps.find((app) => app.id === selectedId) ?? null;

  const refresh = useCallback(async (showSpinner = false) => {
    if (showSpinner) setLoading(true);
    try {
      const [next, status] = await Promise.all([
        invoke<UpdaterApp[]>("list_updater_apps"),
        invoke<MaintenanceView>("load_maintenance_status"),
      ]);
      setApps(Array.isArray(next) ? next : []);
      setMaintenance(status);
    } catch (error) {
      toast.error(errorMessage(error));
    } finally {
      if (showSpinner) setLoading(false);
    }
  }, []);

  const loadLog = useCallback(async (id: string | null) => {
    if (!id) {
      setLogView(null);
      return;
    }
    setLogLoading(true);
    try {
      setLogView(await invoke<UpdaterLogView>("read_updater_log", { app: id }));
    } catch (error) {
      setLogView({ path: "", text: errorMessage(error) });
    } finally {
      setLogLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh(true);
  }, [refresh]);

  useEffect(() => {
    void loadLog(selectedId);
  }, [selectedId, loadLog]);

  async function run(appId: string | null, check: boolean) {
    if (running) return;
    setRunning(check ? "check" : "update");
    try {
      const result = await invoke<UpdaterRunOutcome[]>("run_updater", {
        app: appId,
        check,
      });
      setOutcomes(result);
      const failed = result.filter(
        (item) => item.status === "failed" || item.forbidden,
      ).length;
      if (failed > 0) {
        toast.error(`Updater finished with ${failed} failure(s).`, { duration: 8000 });
      } else {
        toast.success(check ? "Check finished." : "Updater finished.");
      }
      await refresh();
      await loadLog(selectedId);
    } catch (error) {
      toast.error(errorMessage(error), { duration: 10000 });
    } finally {
      setRunning(null);
    }
  }

  async function testApp(app: UpdaterApp) {
    try {
      const text = await invoke<string>("test_updater_app", { id: app.id });
      setTestDialog({ open: true, title: `Test ${app.id}`, text, error: false });
    } catch (error) {
      setTestDialog({
        open: true,
        title: `Test ${app.id}`,
        text: errorMessage(error),
        error: true,
      });
    }
  }

  async function previewPlan(app: UpdaterApp) {
    if (planDialog.loading) return;
    const request = planRequest.current + 1;
    planRequest.current = request;
    setPlanDialog({ open: true, loading: true, appId: app.id, plan: null, error: "" });
    try {
      const result = await invoke<AppPlan>("plan_updater_app", { appId: app.id });
      if (planRequest.current !== request) return;
      setPlanDialog({
        open: true,
        loading: false,
        appId: app.id,
        plan: result,
        error: "",
      });
    } catch (error) {
      if (planRequest.current !== request) return;
      setPlanDialog({
        open: true,
        loading: false,
        appId: app.id,
        plan: null,
        error: errorMessage(error),
      });
    }
  }

  function closePlanDialog() {
    planRequest.current += 1;
    setPlanDialog((previous) => ({
      ...previous,
      open: false,
      loading: false,
    }));
  }

  async function openHistory(app: UpdaterApp) {
    const request = historyRequest.current + 1;
    historyRequest.current = request;
    setHistoryDialog({
      open: true,
      loading: true,
      appId: app.id,
      entries: [],
      error: "",
    });
    try {
      const entries = await invoke<UpdaterHistoryEntry[]>("load_updater_history", {
        app: app.id,
        limit: 50,
      });
      if (historyRequest.current !== request) return;
      setHistoryDialog({
        open: true,
        loading: false,
        appId: app.id,
        entries: Array.isArray(entries) ? entries : [],
        error: "",
      });
    } catch (error) {
      if (historyRequest.current !== request) return;
      setHistoryDialog((previous) => ({
        ...previous,
        loading: false,
        error: errorMessage(error),
      }));
    }
  }

  function closeHistoryDialog() {
    historyRequest.current += 1;
    setHistoryDialog((previous) => ({
      ...previous,
      open: false,
      loading: false,
    }));
  }

  async function confirmDelete() {
    const target = deleteTarget;
    if (!target) return;
    setDeleteTarget(null);
    try {
      const next = await invoke<UpdaterApp[]>("delete_updater_app", { id: target.id });
      setApps(next);
      if (selectedId === target.id) setSelectedId(null);
      toast.success(`App '${target.id}' removed.`);
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }

  function handleSaved(next: UpdaterApp[], id: string, edited: boolean) {
    setApps(next);
    setSelectedId(id);
    toast.success(edited ? "Updater app updated." : "Updater app added.");
  }

  return (
    <>
      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
        <div className="flex flex-wrap items-center gap-2 rounded-lg border bg-card px-3 py-2">
          <div className="mr-auto">
            <h2 className="text-sm font-semibold">Updater Apps</h2>
            <p className="text-xs text-muted-foreground">
              Manual SSH/GUI trigger only. No background polling.
            </p>
          </div>
          <Hint label="Reload the app registry and status">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void refresh(true)}
              disabled={loading || running !== null}
            >
              {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              Refresh
            </Button>
          </Hint>
          <Hint label="Register a new application">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => setForm({ open: true, edit: false, app: null })}
              disabled={running !== null}
            >
              <Plus />
              Add App
            </Button>
          </Hint>
          <Hint label="Check all registered apps without changing files. Uses Max concurrent updates; Update All remains serial.">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void run(null, true)}
              disabled={running !== null || apps.length === 0}
            >
              {running === "check" ? <Loader2 className="animate-spin" /> : <Search />}
              Check All
            </Button>
          </Hint>
          <Hint label="Update all apps; services may restart">
            <Button
              type="button"
              size="sm"
              onClick={() => void run(null, false)}
              disabled={running !== null || apps.length === 0}
            >
              {running === "update" ? <Loader2 className="animate-spin" /> : <Play />}
              Update All
            </Button>
          </Hint>
        </div>

        {maintenance && maintenance.enabled && (
          <div className="flex items-start gap-3 rounded-lg border border-amber-400/70 bg-amber-50 px-4 py-3 text-amber-900 dark:border-amber-500/40 dark:bg-amber-950/40 dark:text-amber-100">
            <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0" />
            <div className="min-w-0">
              <div className="text-sm font-semibold">Maintenance mode is enabled</div>
              <p className="text-sm">
                Updates and the SSH trigger are paused. Check and dry-run plan still work.
              </p>
              {maintenance.message.trim() && (
                <p className="mt-1 break-words text-sm font-medium">{maintenance.message}</p>
              )}
              <p className="mt-1 text-xs">
                Disable maintenance mode in Settings when ready.
              </p>
            </div>
          </div>
        )}

        <div className="min-h-0 flex-[3] overflow-auto rounded-lg border bg-card">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>App</TableHead>
                <TableHead>Branch</TableHead>
                <TableHead>Toolchain</TableHead>
                <TableHead>Services</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Last update</TableHead>
                <TableHead className="w-[124px] text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {apps.length === 0 && (
                <TableRow>
                  <TableCell colSpan={7} className="py-10 text-center text-muted-foreground">
                    No apps registered. Add an app to start.
                  </TableCell>
                </TableRow>
              )}
              {apps.map((app) => (
                <ContextMenu key={app.id}>
                  <ContextMenuTrigger asChild>
                    <TableRow
                      key={app.id}
                      data-state={app.id === selectedId ? "selected" : undefined}
                      className="cursor-pointer"
                      onClick={() => setSelectedId(app.id)}
                      onContextMenu={() => setSelectedId(app.id)}
                    >
                  <TableCell>
                    <div className="font-medium">{app.name || app.id}</div>
                    <div className="text-xs text-muted-foreground">{app.id}</div>
                    {!app.enabled && <div className="text-xs text-amber-600">disabled</div>}
                  </TableCell>
                  <TableCell>{app.branch}</TableCell>
                  <TableCell>
                    <div>{app.preset}</div>
                    <div className="text-xs text-muted-foreground">
                      {app.packageManager} · build {app.build}
                    </div>
                    {app.preset === "laravel" && (
                      <div className="text-xs text-muted-foreground">
                        PHP {app.phpVersion || "version not tested"}
                      </div>
                    )}
                    {app.webServerService && (
                      <Hint
                        label={
                          app.apacheVersion
                            ? `Apache service ${app.webServerService} · ${app.apacheVersion}`
                            : `Apache service ${app.webServerService}`
                        }
                      >
                        <div className="text-xs text-muted-foreground">
                          Web server {app.webServerService}
                          {app.apacheVersion ? ` · ${app.apacheVersion}` : ""}
                        </div>
                      </Hint>
                    )}
                  </TableCell>
                  <TableCell className="max-w-56 truncate text-xs">
                    <TooltipText text={serviceSummary(app)}>{serviceSummary(app)}</TooltipText>
                  </TableCell>
                  <TableCell>
                    <UpdaterStatusBadge status={app.status} />
                    {app.lastError && (
                      <TooltipText
                        text={app.lastError}
                        className="mt-1 max-w-56 text-xs text-destructive"
                      >
                        {app.lastError}
                      </TooltipText>
                    )}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {formatWhen(app.lastUpdatedAt)}
                  </TableCell>
                  <TableCell className="w-[124px] whitespace-nowrap">
                    <div className="flex items-center justify-end gap-0.5">
                      <Hint label="Preview Plan: dry run that fetches remote refs without merging, building, or changing services">
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          aria-label="Preview Plan"
                          disabled={running !== null || planDialog.loading}
                          onClick={(event) => {
                            event.stopPropagation();
                            void previewPlan(app);
                          }}
                        >
                          {planDialog.loading && planDialog.appId === app.id ? (
                            <Loader2 className="animate-spin" />
                          ) : (
                            <ClipboardList />
                          )}
                        </Button>
                      </Hint>
                      <Hint label="Check: verify repository state and preview results without changing files or services">
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          aria-label="Check"
                          disabled={running !== null}
                          onClick={(event) => {
                            event.stopPropagation();
                            void run(app.id, true);
                          }}
                        >
                          <Search />
                        </Button>
                      </Hint>
                      <DropdownMenu>
                        <Hint label="More actions: run, test, log, history, edit, or delete this app">
                          <DropdownMenuTrigger asChild>
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon"
                              className="h-8 w-8"
                              aria-label="More actions"
                              onClick={(event) => event.stopPropagation()}
                            >
                              <MoreHorizontal />
                            </Button>
                          </DropdownMenuTrigger>
                        </Hint>
                        <DropdownMenuContent
                          align="end"
                          className="w-48"
                          onClick={(event) => event.stopPropagation()}
                        >
                          <DropdownMenuItem
                            disabled={running !== null}
                            onSelect={() => void run(app.id, false)}
                          >
                            <Play />
                            Run
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => void testApp(app)}>
                            <FlaskConical />
                            Test
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => setSelectedId(app.id)}>
                            <FileText />
                            Log
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => void openHistory(app)}>
                            <History />
                            History
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => setForm({ open: true, edit: true, app })}>
                            <Pencil />
                            Edit
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            className="text-destructive focus:bg-destructive/10 focus:text-destructive"
                            onSelect={() => setDeleteTarget(app)}
                          >
                            <Trash2 />
                            Delete
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>
                  </TableCell>
                    </TableRow>
                  </ContextMenuTrigger>
                  <ContextMenuContent className="w-48">
                    <ContextMenuItem
                      disabled={running !== null || planDialog.loading}
                      onSelect={() => void previewPlan(app)}
                    >
                      {planDialog.loading && planDialog.appId === app.id ? (
                        <Loader2 className="animate-spin" />
                      ) : (
                        <ClipboardList />
                      )}
                      {planDialog.loading && planDialog.appId === app.id
                        ? "Planning…"
                        : "Preview Plan"}
                    </ContextMenuItem>
                    <ContextMenuItem
                      disabled={running !== null}
                      onSelect={() => void run(app.id, false)}
                    >
                      <Play />
                      Run
                    </ContextMenuItem>
                    <ContextMenuItem
                      disabled={running !== null}
                      onSelect={() => void run(app.id, true)}
                    >
                      <Search />
                      Check
                    </ContextMenuItem>
                    <ContextMenuItem onSelect={() => void testApp(app)}>
                      <FlaskConical />
                      Test
                    </ContextMenuItem>
                    <ContextMenuItem onSelect={() => setSelectedId(app.id)}>
                      <FileText />
                      Log
                    </ContextMenuItem>
                    <ContextMenuItem onSelect={() => void openHistory(app)}>
                      <History />
                      History
                    </ContextMenuItem>
                    <ContextMenuSeparator />
                    <ContextMenuItem
                      onSelect={() => setForm({ open: true, edit: true, app })}
                    >
                      <Pencil />
                      Edit
                    </ContextMenuItem>
                    <ContextMenuItem
                      className="text-destructive focus:bg-destructive/10 focus:text-destructive"
                      onSelect={() => setDeleteTarget(app)}
                    >
                      <Trash2 />
                      Delete
                    </ContextMenuItem>
                  </ContextMenuContent>
                </ContextMenu>
              ))}
            </TableBody>
          </Table>
        </div>

        <div className="flex min-h-0 flex-[2] flex-col overflow-hidden rounded-lg border bg-card">
          <div className="flex items-center justify-between gap-2 border-b px-3 py-2">
            <div className="min-w-0">
              <div className="text-sm font-medium">
                {selected ? `Log: ${selected.id}` : "Updater log"}
              </div>
              <TooltipText
                text={logView?.path ?? ""}
                className="text-xs text-muted-foreground"
              >
                {logView?.path || "Select an app to show its latest log."}
              </TooltipText>
            </div>
            <Hint label="Reload the latest log for the selected app">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void loadLog(selectedId)}
                disabled={!selectedId || logLoading}
              >
                {logLoading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
                Reload
              </Button>
            </Hint>
          </div>
          <pre className="min-h-24 flex-1 overflow-auto whitespace-pre-wrap break-words p-3 font-mono text-xs">
            {logView?.text || "No log loaded."}
          </pre>
        </div>
      </div>

      <UpdaterFormDialog
        open={form.open}
        edit={form.edit}
        app={form.app}
        onOpenChange={(open) => setForm((previous) => ({ ...previous, open }))}
        onSaved={handleSaved}
      />

      <ModalShell
        open={planDialog.open}
        onOpenChange={(open) => {
          if (open) {
            setPlanDialog((previous) => ({ ...previous, open: true }));
          } else {
            closePlanDialog();
          }
        }}
        title={`Update plan: ${planDialog.plan?.appName || planDialog.appId}`}
        description="Dry run. Plan fetches remote refs but does not merge, build, install, or change services."
        icon={<ClipboardList className="h-5 w-5 text-primary" />}
        maxWidthClass="sm:max-w-3xl"
        footer={
          <Button type="button" variant="outline" onClick={closePlanDialog}>
            Close
          </Button>
        }
      >
        {planDialog.loading && (
          <div className="flex items-center gap-2 py-8 text-sm text-muted-foreground">
            <Loader2 className="animate-spin" />
            Generating dry-run plan…
          </div>
        )}

        {planDialog.error && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {planDialog.error}
          </div>
        )}

        {planDialog.plan && <PlanDetails plan={planDialog.plan} />}
      </ModalShell>

      <ModalShell
        open={historyDialog.open}
        onOpenChange={(open) => {
          if (open) {
            setHistoryDialog((previous) => ({ ...previous, open: true }));
          } else {
            closeHistoryDialog();
          }
        }}
        title={`History: ${historyDialog.appId}`}
        description="Latest 50 update outcomes, newest first. History is append-only and is not removed by log retention."
        icon={<History className="h-5 w-5 text-primary" />}
        maxWidthClass="sm:max-w-2xl"
        footer={
          <Button type="button" variant="outline" onClick={closeHistoryDialog}>
            Close
          </Button>
        }
      >
        {historyDialog.loading && (
          <div className="flex items-center gap-2 py-8 text-sm text-muted-foreground">
            <Loader2 className="animate-spin" />
            Loading history…
          </div>
        )}

        {historyDialog.error && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {historyDialog.error}
          </div>
        )}

        {!historyDialog.loading &&
          !historyDialog.error &&
          historyDialog.entries.length === 0 && (
            <p className="py-6 text-center text-sm text-muted-foreground">No history yet.</p>
          )}

        {historyDialog.entries.length > 0 && (
          <div className="space-y-2">
            {historyDialog.entries.map((entry, index) => (
              <div
                key={`${entry.timestamp}-${index}`}
                className="rounded-md border px-3 py-2 text-xs"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <UpdaterStatusBadge status={entry.status} />
                  <span className="text-muted-foreground">{formatWhen(entry.timestamp)}</span>
                  <TooltipText
                    text={entry.commit || "No commit recorded"}
                    className="ml-auto font-mono text-muted-foreground"
                  >
                    {shortSha(entry.commit)}
                  </TooltipText>
                </div>
                <p className="mt-1 whitespace-pre-wrap break-words text-sm">
                  {entry.message || "—"}
                </p>
                <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1 text-muted-foreground">
                  <span>Duration: {formatDuration(entry.durationMs)}</span>
                  <Hint label="Trigger client IP recorded for audit">
                    <span>Client IP: {entry.clientIp || "—"}</span>
                  </Hint>
                </div>
              </div>
            ))}
          </div>
        )}
      </ModalShell>

      <ModalShell
        open={outcomes !== null}
        onOpenChange={(open) => {
          if (!open) setOutcomes(null);
        }}
        title="Updater result"
        description="Latest run for the selected app(s)."
        icon={<ListChecks className="h-5 w-5 text-primary" />}
        maxWidthClass="sm:max-w-2xl"
        bodyClassName="space-y-2"
        footer={
          <Button type="button" variant="outline" onClick={() => setOutcomes(null)}>
            Close
          </Button>
        }
      >
        {(outcomes ?? []).map((outcome) => (
          <div key={outcome.appId} className="rounded-md border px-3 py-2">
            <div className="flex items-center justify-between gap-2">
              <span className="font-medium">{outcome.appId}</span>
              <UpdaterStatusBadge status={outcome.status} />
            </div>
            <p className="mt-1 text-sm text-muted-foreground">{outcome.message}</p>
          </div>
        ))}
      </ModalShell>

      <ModalShell
        open={testDialog.open}
        onOpenChange={(open) => setTestDialog((previous) => ({ ...previous, open }))}
        title={testDialog.title}
        description="Path, branch, remote, and toolchain detection."
        icon={<FlaskConical className="h-5 w-5 text-primary" />}
        maxWidthClass="sm:max-w-2xl"
        footer={
          <Button
            type="button"
            variant="outline"
            onClick={() => setTestDialog((previous) => ({ ...previous, open: false }))}
          >
            Close
          </Button>
        }
      >
        <pre
          className={`whitespace-pre-wrap break-words rounded-md border p-3 font-mono text-xs ${
            testDialog.error ? "text-destructive" : ""
          }`}
        >
          {testDialog.text}
        </pre>
      </ModalShell>

      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
        title="Remove updater app?"
        description={`This removes ${deleteTarget?.id ?? "the app"} from the registry. Project files are not deleted.`}
        icon={<Trash2 className="h-5 w-5 text-destructive" />}
        destructive
        confirmLabel="Remove"
        onConfirm={() => void confirmDelete()}
      />
    </>
  );
}
