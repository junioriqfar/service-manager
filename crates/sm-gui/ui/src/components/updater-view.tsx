import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  ClipboardList,
  FileText,
  FlaskConical,
  History,
  Loader2,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";

import { UpdaterFormDialog } from "@/components/updater-form-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
  PlanCommandGroup,
  PlanService,
  UpdaterApp,
  UpdaterHistoryEntry,
  UpdaterLogView,
  UpdaterRunOutcome,
} from "@/lib/types";

function statusVariant(status: string): "default" | "secondary" | "destructive" | "success" | "warning" | "outline" {
  switch (status) {
    case "updated":
    case "ready":
      return "success";
    case "up-to-date":
      return "secondary";
    case "check":
      return "outline";
    case "dirty":
    case "blocked-migration":
    case "clone-required":
    case "degraded":
      return "warning";
    case "failed":
    case "forbidden":
    case "error":
      return "destructive";
    default:
      return "outline";
  }
}

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
        <Badge
          variant={
            running === true ? "success" : running === false ? "secondary" : "warning"
          }
        >
          {running === true ? "running" : running === false ? "stopped" : "unknown"}
        </Badge>
      </div>
      {service.error && <div className="mt-1 text-destructive">{service.error}</div>}
    </li>
  );
}

function PlanDetails({ plan }: { plan: AppPlan }) {
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant={statusVariant(plan.status)}>{plan.status}</Badge>
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
      const next = await invoke<UpdaterApp[]>("list_updater_apps");
      setApps(Array.isArray(next) ? next : []);
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
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void refresh(true)}
            disabled={loading || running !== null}
            title="Reload the app registry and status"
          >
            {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            Refresh
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setForm({ open: true, edit: false, app: null })}
            disabled={running !== null}
            title="Register a new application"
          >
            <Plus />
            Add App
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void run(null, true)}
            disabled={running !== null || apps.length === 0}
            title="Check all registered apps without changing files"
          >
            {running === "check" ? <Loader2 className="animate-spin" /> : <Search />}
            Check All
          </Button>
          <Button
            type="button"
            size="sm"
            onClick={() => void run(null, false)}
            disabled={running !== null || apps.length === 0}
            title="Update all apps; services may restart"
          >
            {running === "update" ? <Loader2 className="animate-spin" /> : <Play />}
            Update All
          </Button>
        </div>

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
                <TableHead className="text-right">Actions</TableHead>
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
                      <div
                        className="text-xs text-muted-foreground"
                        title={
                          app.apacheVersion
                            ? `Apache service ${app.webServerService} · ${app.apacheVersion}`
                            : `Apache service ${app.webServerService}`
                        }
                      >
                        Web server {app.webServerService}
                        {app.apacheVersion ? ` · ${app.apacheVersion}` : ""}
                      </div>
                    )}
                  </TableCell>
                  <TableCell className="max-w-56 truncate text-xs" title={serviceSummary(app)}>
                    {serviceSummary(app)}
                  </TableCell>
                  <TableCell>
                    <Badge variant={statusVariant(app.status)}>{app.status}</Badge>
                    {app.lastError && (
                      <div className="mt-1 max-w-56 truncate text-xs text-destructive" title={app.lastError}>
                        {app.lastError}
                      </div>
                    )}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {formatWhen(app.lastUpdatedAt)}
                  </TableCell>
                  <TableCell>
                    <div className="flex flex-wrap justify-end gap-1">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title="Run"
                        disabled={running !== null}
                        onClick={(event) => {
                          event.stopPropagation();
                          void run(app.id, false);
                        }}
                      >
                        <Play />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title="Check"
                        disabled={running !== null}
                        onClick={(event) => {
                          event.stopPropagation();
                          void run(app.id, true);
                        }}
                      >
                        <Search />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title="Test"
                        onClick={(event) => {
                          event.stopPropagation();
                          void testApp(app);
                        }}
                      >
                        <FlaskConical />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title="Log"
                        onClick={(event) => {
                          event.stopPropagation();
                          setSelectedId(app.id);
                        }}
                      >
                        <FileText />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title="Edit"
                        onClick={(event) => {
                          event.stopPropagation();
                          setForm({ open: true, edit: true, app });
                        }}
                      >
                        <Pencil />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title="Delete"
                        onClick={(event) => {
                          event.stopPropagation();
                          setDeleteTarget(app);
                        }}
                      >
                        <Trash2 />
                      </Button>
                    </div>
                  </TableCell>
                    </TableRow>
                  </ContextMenuTrigger>
                  <ContextMenuContent className="w-48">
                    <ContextMenuItem
                      disabled={running !== null || planDialog.loading}
                      title="Dry run: fetch remote refs and preview the plan without merging or changing services"
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
                    <ContextMenuItem
                      title="Show the latest 50 update history entries for this app"
                      onSelect={() => void openHistory(app)}
                    >
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
              <div className="truncate text-xs text-muted-foreground" title={logView?.path ?? ""}>
                {logView?.path || "Select an app to show its latest log."}
              </div>
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void loadLog(selectedId)}
              disabled={!selectedId || logLoading}
              title="Reload the latest log for the selected app"
            >
              {logLoading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              Reload
            </Button>
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

      <Dialog
        open={planDialog.open}
        onOpenChange={(open) => {
          if (open) {
            setPlanDialog((previous) => ({ ...previous, open: true }));
          } else {
            closePlanDialog();
          }
        }}
      >
        <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-3xl">
          <DialogHeader>
            <DialogTitle>
              Update plan: {planDialog.plan?.appName || planDialog.appId}
            </DialogTitle>
            <DialogDescription>
              Dry run. Plan fetches remote refs but does not merge, build, install, or
              change services.
            </DialogDescription>
          </DialogHeader>

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

          <DialogFooter>
            <Button type="button" variant="outline" onClick={closePlanDialog}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={historyDialog.open}
        onOpenChange={(open) => {
          if (open) {
            setHistoryDialog((previous) => ({ ...previous, open: true }));
          } else {
            closeHistoryDialog();
          }
        }}
      >
        <DialogContent className="max-h-[85vh] sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>History: {historyDialog.appId}</DialogTitle>
            <DialogDescription>
              Latest 50 update outcomes, newest first. History is append-only and is not
              removed by log retention.
            </DialogDescription>
          </DialogHeader>

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
            <div className="max-h-[60vh] space-y-2 overflow-y-auto pr-1">
              {historyDialog.entries.map((entry, index) => (
                <div
                  key={`${entry.timestamp}-${index}`}
                  className="rounded-md border px-3 py-2 text-xs"
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant={statusVariant(entry.status)}>{entry.status}</Badge>
                    <span className="text-muted-foreground">{formatWhen(entry.timestamp)}</span>
                    <span
                      className="ml-auto font-mono text-muted-foreground"
                      title={entry.commit || "No commit recorded"}
                    >
                      {shortSha(entry.commit)}
                    </span>
                  </div>
                  <p className="mt-1 whitespace-pre-wrap break-words text-sm">
                    {entry.message || "—"}
                  </p>
                  <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1 text-muted-foreground">
                    <span>Duration: {formatDuration(entry.durationMs)}</span>
                    <span title="Trigger client IP recorded for audit">
                      Client IP: {entry.clientIp || "—"}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          )}

          <DialogFooter>
            <Button type="button" variant="outline" onClick={closeHistoryDialog}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={outcomes !== null}
        onOpenChange={(open) => {
          if (!open) setOutcomes(null);
        }}
      >
        <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>Updater result</DialogTitle>
            <DialogDescription>Latest run for the selected app(s).</DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            {(outcomes ?? []).map((outcome) => (
              <div key={outcome.appId} className="rounded-md border px-3 py-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="font-medium">{outcome.appId}</span>
                  <Badge variant={statusVariant(outcome.status)}>{outcome.status}</Badge>
                </div>
                <p className="mt-1 text-sm text-muted-foreground">{outcome.message}</p>
              </div>
            ))}
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOutcomes(null)}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={testDialog.open}
        onOpenChange={(open) => setTestDialog((previous) => ({ ...previous, open }))}
      >
        <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{testDialog.title}</DialogTitle>
            <DialogDescription>Path, branch, remote, and toolchain detection.</DialogDescription>
          </DialogHeader>
          <pre
            className={`whitespace-pre-wrap break-words rounded-md border p-3 font-mono text-xs ${
              testDialog.error ? "text-destructive" : ""
            }`}
          >
            {testDialog.text}
          </pre>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setTestDialog((previous) => ({ ...previous, open: false }))}
            >
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Remove updater app?</DialogTitle>
            <DialogDescription>
              This removes {deleteTarget?.id ?? "the app"} from the registry. Project files are not deleted.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setDeleteTarget(null)}>
              Cancel
            </Button>
            <Button type="button" variant="destructive" onClick={() => void confirmDelete()}>
              Remove
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
