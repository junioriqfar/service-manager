import { useCallback, useEffect, useRef, useState, type ChangeEvent } from "react";
import {
  Download,
  FileText,
  FolderOpen,
  Info,
  PackageOpen,
  Play,
  RadioTower,
  RotateCcw,
  ServerCog,
  Settings as SettingsIcon,
  Square,
  Upload,
} from "lucide-react";
import { toast } from "sonner";

import { AboutDialog } from "@/components/about-dialog";
import { AdminBanner } from "@/components/admin-banner";
import { BatchDialog } from "@/components/batch-dialog";
import { Hint } from "@/components/hint";
import { ImportDialog } from "@/components/import-dialog";
import { LogPanel } from "@/components/log-panel";
import { RemoteTriggerView } from "@/components/remote-trigger-view";
import { DeleteServiceDialog, InstallServiceDialog } from "@/components/service-action-dialogs";
import { ServiceFormDialog } from "@/components/service-form-dialog";
import { ServiceTable } from "@/components/service-table";
import { SettingsView } from "@/components/settings-view";
import { ThemeProvider } from "@/components/theme-provider";
import { ThemeToggle } from "@/components/theme-toggle";
import { UpdaterView } from "@/components/updater-view";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { batchSummaryText, countsLabel } from "@/lib/service";
import { errorMessage, invoke, listen } from "@/lib/tauri";
import type {
  BatchActionName,
  BatchProgressEvent,
  BatchStartedEvent,
  BatchSummary,
  BatchView,
  LogView,
  PathsView,
  ServiceAction,
  ServiceConfig,
  ServiceRow,
} from "@/lib/types";

type SingleActionCommand = "start_service" | "stop_service" | "restart_service";

function App() {
  return (
    <ThemeProvider>
      <TooltipProvider delayDuration={250}>
        <ServiceManager />
        <Toaster />
      </TooltipProvider>
    </ThemeProvider>
  );
}

function ServiceManager() {
  const [view, setView] = useState<"services" | "updater" | "remote" | "settings">("services");
  const [services, setServices] = useState<ServiceRow[]>([]);
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [admin, setAdmin] = useState<boolean | null>(null);
  const [configHome, setConfigHome] = useState("");
  const [statusText, setStatusText] = useState("Ready");
  const [refreshing, setRefreshing] = useState(false);
  const [pendingService, setPendingService] = useState<string | null>(null);

  const [batchRunning, setBatchRunning] = useState(false);
  const [batch, setBatch] = useState<BatchView | null>(null);

  const [logView, setLogView] = useState<LogView | null>(null);
  const [logAutoRefresh, setLogAutoRefresh] = useState(true);

  const [serviceDialog, setServiceDialog] = useState<{
    open: boolean;
    edit: boolean;
    service: ServiceRow | null;
  }>({ open: false, edit: false, service: null });

  const [importOpen, setImportOpen] = useState(false);
  const [importLoading, setImportLoading] = useState(false);
  const [importCandidates, setImportCandidates] = useState<ServiceConfig[]>([]);
  const [importing, setImporting] = useState(false);

  const [aboutOpen, setAboutOpen] = useState(false);
  const [paths, setPaths] = useState<PathsView | null>(null);

  const [deleteTarget, setDeleteTarget] = useState<ServiceRow | null>(null);
  const [installTarget, setInstallTarget] = useState<ServiceRow | null>(null);

  const refreshingRef = useRef(false);
  const logOutputRef = useRef<HTMLPreElement>(null);
  const restoreInputRef = useRef<HTMLInputElement>(null);
  const batchCloseTimer = useRef<number | null>(null);

  const selectedService = services.find((service) => service.name === selectedName) ?? null;
  const hasInstalled = services.some((service) => service.installed);
  const counts = countsLabel(services);

  const refresh = useCallback(async (showSpinner = false) => {
    if (refreshingRef.current) return;
    refreshingRef.current = true;
    if (showSpinner) setRefreshing(true);
    try {
      const nextServices = await invoke<ServiceRow[]>("list_services");
      setServices(Array.isArray(nextServices) ? nextServices : []);
    } catch (error) {
      toast.error(errorMessage(error));
    } finally {
      refreshingRef.current = false;
      if (showSpinner) setRefreshing(false);
    }
  }, []);

  const loadLog = useCallback(async (name: string, force: boolean) => {
    try {
      const result = await invoke<LogView>("read_log", { name });
      const output = logOutputRef.current;
      const nearBottom = output
        ? output.scrollTop + output.clientHeight >= output.scrollHeight - 24
        : true;
      setLogView(result);
      if (force || nearBottom) {
        window.requestAnimationFrame(() => {
          const current = logOutputRef.current;
          if (current) current.scrollTop = current.scrollHeight;
        });
      }
    } catch (error) {
      setLogView({ path: "(no log path)", text: errorMessage(error) });
    }
  }, []);

  useEffect(() => {
    let active = true;

    async function initialize() {
      try {
        const isAdmin = await invoke<boolean>("is_admin");
        if (active) setAdmin(isAdmin);
      } catch (error) {
        if (active) toast.error(errorMessage(error));
      }

      try {
        const nextPaths = await invoke<PathsView>("get_paths");
        if (active) {
          setPaths(nextPaths);
          setConfigHome(nextPaths.home);
        }
      } catch (error) {
        if (active) toast.error(errorMessage(error));
      }

      await refresh();
      if (!active) return;

      try {
        const warning = await invoke<string | null>("get_warning");
        if (warning) toast.error(warning, { duration: 10000 });
      } catch (error) {
        toast.error(errorMessage(error));
      }
    }

    void initialize();
    return () => {
      active = false;
    };
  }, [refresh]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    async function setup() {
      try {
        const started = await listen<BatchStartedEvent>("batch-started", (event) => {
          const total = Math.max(1, event.payload.total || 1);
          setBatchRunning(true);
          setBatch((previous) =>
            previous
              ? { ...previous, open: true, total, done: 0 }
              : {
                  open: true,
                  action: event.payload.action,
                  total,
                  done: 0,
                  text: "Preparing...",
                  lines: [],
                  cancelling: false,
                  finished: false,
                  summary: null,
                },
          );
        });

        const progress = await listen<BatchProgressEvent>("batch-progress", (event) => {
          const payload = event.payload;
          const total = Math.max(1, payload.total || 1);
          const message = payload.message ?? null;
          const suffix = message ? ` - ${message}` : "";
          setBatch((previous) =>
            previous
              ? {
                  ...previous,
                  total,
                  done: payload.done || 0,
                  text: `${payload.done}/${total} ${payload.name}: ${payload.state}${suffix}`,
                  lines: [...previous.lines, { state: payload.state, name: payload.name, message }],
                }
              : previous,
          );
        });

        const finished = await listen<BatchSummary>("batch-finished", (event) => {
          setBatchRunning(false);
          setBatch((previous) =>
            previous ? { ...previous, finished: true, summary: event.payload } : previous,
          );
        });

        if (disposed) {
          started();
          progress();
          finished();
        } else {
          unlisteners.push(started, progress, finished);
        }
      } catch (error) {
        console.error("Failed to register batch listeners.", error);
      }
    }

    void setup();
    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => {
      if (view === "services" && !batchRunning && document.visibilityState === "visible") {
        void refresh();
      }
    }, 3000);
    return () => window.clearInterval(timer);
  }, [batchRunning, refresh, view]);

  useEffect(() => {
    if (!selectedName) {
      setLogView(null);
      return;
    }
    void loadLog(selectedName, true);
  }, [selectedName, loadLog]);

  useEffect(() => {
    if (!logAutoRefresh || !selectedName) return;
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void loadLog(selectedName, false);
    }, 2000);
    return () => window.clearInterval(timer);
  }, [logAutoRefresh, selectedName, loadLog]);

  useEffect(() => {
    if (selectedName && !services.some((service) => service.name === selectedName)) {
      setSelectedName(null);
    }
  }, [services, selectedName]);

  useEffect(() => {
    return () => {
      if (batchCloseTimer.current !== null) {
        window.clearTimeout(batchCloseTimer.current);
      }
    };
  }, []);

  function scheduleBatchClose(delay = 1500) {
    if (batchCloseTimer.current !== null) {
      window.clearTimeout(batchCloseTimer.current);
    }
    batchCloseTimer.current = window.setTimeout(() => {
      setBatch((previous) => (previous ? { ...previous, open: false } : previous));
      batchCloseTimer.current = null;
    }, delay);
  }

  function closeBatch() {
    if (batchRunning) return;
    if (batchCloseTimer.current !== null) {
      window.clearTimeout(batchCloseTimer.current);
      batchCloseTimer.current = null;
    }
    setBatch((previous) => (previous ? { ...previous, open: false } : previous));
  }

  async function runBatch(action: BatchActionName) {
    if (batchRunning) return;
    if (batchCloseTimer.current !== null) {
      window.clearTimeout(batchCloseTimer.current);
      batchCloseTimer.current = null;
    }

    setBatchRunning(true);
    setBatch({
      open: true,
      action,
      total: 0,
      done: 0,
      text: "Preparing...",
      lines: [],
      cancelling: false,
      finished: false,
      summary: null,
    });

    try {
      const summary = await invoke<BatchSummary>("run_batch_action", { action });
      setBatch((previous) => {
        if (!previous) return previous;
        const total = Math.max(previous.total, summary.outcomes.length, 1);
        return {
          ...previous,
          total,
          done: Math.max(previous.done, total),
          text: batchSummaryText(summary),
          summary,
          finished: true,
        };
      });
      if (summary.failed > 0) {
        toast.error("Batch operation completed.", { duration: 8000 });
      } else {
        toast.success("Batch operation completed.");
      }
      if (summary.failed === 0 && summary.cancelled === 0) {
        scheduleBatchClose();
      }
    } catch (error) {
      const message = errorMessage(error);
      setBatch((previous) =>
        previous ? { ...previous, text: message, finished: true } : previous,
      );
      toast.error(message, { duration: 8000 });
      scheduleBatchClose();
    } finally {
      setBatchRunning(false);
      await refresh();
    }
  }

  function handleCancelBatch() {
    setBatch((previous) =>
      previous
        ? {
            ...previous,
            cancelling: true,
            text: "Cancelling... running operations will finish.",
          }
        : previous,
    );
    void invoke("cancel_batch").catch((error) => toast.error(errorMessage(error)));
  }

  async function runSingleAction(command: SingleActionCommand, service: ServiceRow) {
    const action = command.replace("_service", "");
    setSelectedName(service.name);
    setPendingService(service.name);
    setStatusText(`Running ${action} for ${service.name}...`);
    try {
      const nextServices = await invoke<ServiceRow[]>(command, { name: service.name });
      setServices(nextServices);
      setStatusText("Ready");
      toast.success(`${action} ${service.name} completed.`);
      void loadLog(service.name, true);
    } catch (error) {
      setStatusText("Failed");
      toast.error(errorMessage(error), { duration: 8000 });
    } finally {
      setPendingService(null);
    }
  }

  async function handleOpenLog(service: ServiceRow) {
    try {
      await invoke("open_log", { name: service.name });
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }

  async function confirmDelete() {
    const target = deleteTarget;
    if (!target) return;
    setDeleteTarget(null);
    setPendingService(target.name);
    try {
      const nextServices = await invoke<ServiceRow[]>("delete_service", { name: target.name });
      setServices(nextServices);
      setSelectedName(null);
      setLogView(null);
      toast.success(`Service '${target.name}' deleted.`);
    } catch (error) {
      toast.error(errorMessage(error), { duration: 8000 });
    } finally {
      setPendingService(null);
    }
  }

  async function confirmInstall(startAfter: boolean) {
    const target = installTarget;
    if (!target) return;
    setInstallTarget(null);
    setSelectedName(target.name);
    setPendingService(target.name);
    try {
      const nextServices = await invoke<ServiceRow[]>("install_service", {
        name: target.name,
        startAfter,
      });
      setServices(nextServices);
      toast.success(`Service '${target.name}' installed.`);
      void loadLog(target.name, true);
    } catch (error) {
      toast.error(errorMessage(error), { duration: 8000 });
    } finally {
      setPendingService(null);
    }
  }

  function handleRowAction(action: ServiceAction, service: ServiceRow) {
    switch (action) {
      case "install":
        setInstallTarget(service);
        break;
      case "start":
        void runSingleAction("start_service", service);
        break;
      case "stop":
        void runSingleAction("stop_service", service);
        break;
      case "restart":
        void runSingleAction("restart_service", service);
        break;
      case "log":
        void handleOpenLog(service);
        break;
      case "delete":
        setDeleteTarget(service);
        break;
    }
  }

  function handleEdit(service: ServiceRow) {
    setSelectedName(service.name);
    setServiceDialog({ open: true, edit: true, service });
  }

  function handleAdd() {
    setServiceDialog({ open: true, edit: false, service: null });
  }

  function handleServiceSaved(rows: ServiceRow[], name: string, edited: boolean) {
    setServices(rows);
    setSelectedName(name);
    toast.success(edited ? "Service config updated." : "Service added.");
    void loadLog(name, true);
  }

  async function openImport() {
    if (importLoading) return;
    setImportLoading(true);
    try {
      const candidates = await invoke<ServiceConfig[]>("import_candidates");
      if (!candidates || candidates.length === 0) {
        toast("No new services to import.");
        return;
      }
      setImportCandidates(candidates);
      setImportOpen(true);
    } catch (error) {
      toast.error(errorMessage(error), { duration: 8000 });
    } finally {
      setImportLoading(false);
    }
  }

  async function confirmImport(names: string[]) {
    if (names.length === 0) {
      toast.error("Select at least one service.");
      return;
    }
    setImporting(true);
    try {
      const nextServices = await invoke<ServiceRow[]>("import_services", { names });
      setServices(nextServices);
      setImportOpen(false);
      toast.success(`${names.length} service(s) imported.`);
    } catch (error) {
      toast.error(errorMessage(error), { duration: 8000 });
    } finally {
      setImporting(false);
    }
  }

  async function backupConfig() {
    try {
      const path = await invoke<string>("export_config");
      toast.success(`Backup created:\n${path}`, { duration: 8000 });
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }

  async function restoreConfigFile(file: File) {
    try {
      const content = await file.text();
      const nextServices = await invoke<ServiceRow[]>("restore_config", { content });
      setServices(nextServices);
      setSelectedName(null);
      setLogView(null);
      toast.success("Config restored successfully.");
    } catch (error) {
      toast.error(errorMessage(error), { duration: 8000 });
    }
  }

  function handleRestoreChange(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (file) void restoreConfigFile(file);
  }

  async function openAbout() {
    try {
      const nextPaths = await invoke<PathsView>("get_paths");
      setPaths(nextPaths);
      setConfigHome(nextPaths.home);
      setAboutOpen(true);
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }

  function handleElevate() {
    void invoke("elevate").catch((error) => toast.error(errorMessage(error)));
  }

  function handleOpenFolder() {
    void invoke("open_app_folder").catch((error) => toast.error(errorMessage(error)));
  }

  function handleOpenAppLog() {
    void invoke("open_app_log").catch((error) => toast.error(errorMessage(error)));
  }

  return (
    <>
      <div className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
        {admin === false && <AdminBanner onElevate={handleElevate} />}

        <header className="flex flex-wrap items-center gap-x-4 gap-y-3 border-b bg-card/80 px-4 py-3 backdrop-blur">
          <div className="flex min-w-0 items-center gap-3">
            <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm">
              <ServerCog className="h-5 w-5" />
            </div>
            <div className="min-w-0">
              <h1 className="truncate text-base font-semibold leading-tight">Service Manager</h1>
              <p className="truncate text-xs text-muted-foreground" title={configHome}>
                {configHome || "Loading paths..."}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-1 rounded-md border bg-background/60 p-1" role="tablist">
            <Button
              type="button"
              size="sm"
              variant={view === "services" ? "secondary" : "ghost"}
              onClick={() => setView("services")}
              role="tab"
              aria-selected={view === "services"}
              title="Services: install, start, stop, restart Windows services"
            >
              <ServerCog />
              Services
            </Button>
            <Button
              type="button"
              size="sm"
              variant={view === "updater" ? "secondary" : "ghost"}
              onClick={() => setView("updater")}
              role="tab"
              aria-selected={view === "updater"}
              title="Updater: registry apps, toolchain, check/update"
            >
              <PackageOpen />
              Updater
            </Button>
            <Button
              type="button"
              size="sm"
              variant={view === "remote" ? "secondary" : "ghost"}
              onClick={() => setView("remote")}
              role="tab"
              aria-selected={view === "remote"}
              title="Remote: trigger the server over SSH"
            >
              <RadioTower />
              Remote
            </Button>
            <Button
              type="button"
              size="sm"
              variant={view === "settings" ? "secondary" : "ghost"}
              onClick={() => setView("settings")}
              role="tab"
              aria-selected={view === "settings"}
              title="Settings: toolchain paths, updater defaults, and privileged execution"
            >
              <SettingsIcon />
              Settings
            </Button>
          </div>

          <div className="ml-auto flex flex-wrap items-center gap-x-2 gap-y-2">
            {view === "services" && (
              <>
                <div className="flex items-center gap-1.5" role="group" aria-label="Batch actions">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void runBatch("start")}
                    disabled={!hasInstalled || batchRunning}
                    title="Start all installed services in dependency order"
                  >
                    <Play className="text-emerald-600 dark:text-emerald-400" />
                    Start All
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void runBatch("stop")}
                    disabled={!hasInstalled || batchRunning}
                    title="Stop all installed services in reverse dependency order"
                  >
                    <Square className="fill-current text-red-600 dark:text-red-400" />
                    Stop All
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void runBatch("restart")}
                    disabled={!hasInstalled || batchRunning}
                    title="Restart all installed services"
                  >
                    <RotateCcw className="text-blue-600 dark:text-blue-400" />
                    Restart All
                  </Button>
                </div>

                <Separator orientation="vertical" className="mx-1 hidden h-6 sm:block" />

                <div className="flex items-center gap-1.5" role="group" aria-label="Global actions">
                  <Button type="button" variant="outline" size="sm" onClick={() => void backupConfig()} title="Export service config to backup folder">
                    <Download />
                    Backup
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => restoreInputRef.current?.click()}
                    title="Restore service config from a JSON file"
                  >
                    <Upload />
                    Restore
                  </Button>
                  <Button type="button" variant="outline" size="sm" onClick={handleOpenFolder} title="Open config folder">
                    <FolderOpen />
                    Folder
                  </Button>
                  <Button type="button" variant="outline" size="sm" onClick={handleOpenAppLog} title="Open application log">
                    <FileText />
                    App Log
                  </Button>
                  <Button type="button" variant="outline" size="sm" onClick={() => void openAbout()} title="About Service Manager">
                    <Info />
                    About
                  </Button>
                </div>

                <Separator orientation="vertical" className="mx-1 hidden h-6 sm:block" />
              </>
            )}
            <Hint label="Theme: light, dark, or system" side="left">
              <ThemeToggle />
            </Hint>
          </div>
        </header>
        <main className="flex min-h-0 flex-1 flex-col gap-3 p-3">
          {view === "settings" ? (
            <SettingsView admin={admin} />
          ) : view === "updater" ? (
            <UpdaterView />
          ) : view === "remote" ? (
            <RemoteTriggerView />
          ) : (
            <>
              <ServiceTable
                services={services}
                selectedName={selectedName}
                batchRunning={batchRunning}
                pendingService={pendingService}
                refreshing={refreshing}
                importing={importLoading}
                onSelect={setSelectedName}
                onAdd={handleAdd}
                onImport={() => void openImport()}
                onRefresh={() => void refresh(true)}
                onEdit={handleEdit}
                onAction={handleRowAction}
              />

              <LogPanel
                serviceName={selectedName}
                logView={logView}
                autoRefresh={logAutoRefresh}
                onAutoRefreshChange={setLogAutoRefresh}
                onReload={() => {
                  if (selectedName) void loadLog(selectedName, true);
                }}
                onOpenFile={() => {
                  if (selectedService) void handleOpenLog(selectedService);
                }}
                outputRef={logOutputRef}
              />
            </>
          )}
        </main>

        <footer className="flex items-center justify-between gap-4 border-t bg-card px-4 py-1.5 text-xs">
          <span>
            {view === "settings"
              ? "Settings ready"
              : view === "updater"
                ? "Updater ready"
                : view === "remote"
                  ? "Remote trigger ready"
                  : statusText}
          </span>
          <span className="truncate text-muted-foreground">
            {view === "settings"
              ? "Changes are written only when you click Save Settings"
              : view === "updater"
                ? "Manual trigger only — GUI or sm-trigger over SSH"
                : view === "remote"
                  ? "SSH client — keys/agent only, no background polling"
                  : counts}
          </span>
        </footer>
      </div>

      <input
        ref={restoreInputRef}
        type="file"
        accept=".json,application/json"
        className="hidden"
        onChange={handleRestoreChange}
      />

      <ServiceFormDialog
        open={serviceDialog.open}
        edit={serviceDialog.edit}
        service={serviceDialog.service}
        onOpenChange={(open) => setServiceDialog((previous) => ({ ...previous, open }))}
        onSaved={handleServiceSaved}
      />

      <ImportDialog
        open={importOpen}
        candidates={importCandidates}
        importing={importing}
        onOpenChange={setImportOpen}
        onImport={(names) => void confirmImport(names)}
      />

      <AboutDialog open={aboutOpen} paths={paths} onOpenChange={setAboutOpen} />

      <BatchDialog
        batch={batch}
        running={batchRunning}
        onCancel={handleCancelBatch}
        onClose={closeBatch}
      />

      <DeleteServiceDialog
        service={deleteTarget}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
        onConfirm={() => void confirmDelete()}
      />

      <InstallServiceDialog
        service={installTarget}
        onOpenChange={(open) => {
          if (!open) setInstallTarget(null);
        }}
        onConfirm={(startAfter) => void confirmInstall(startAfter)}
      />
    </>
  );
}

export default App;
