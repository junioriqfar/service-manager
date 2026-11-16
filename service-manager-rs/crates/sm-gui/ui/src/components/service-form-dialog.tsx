import { useEffect, useState, type ReactNode } from "react";
import {
  AlertCircle,
  Database,
  FileSearch,
  FileText,
  FolderOpen,
  Loader2,
  Save,
  Server,
} from "lucide-react";

import { Button } from "@/components/ui/button";
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
import { splitDependencies } from "@/lib/service";
import { errorMessage, invoke } from "@/lib/tauri";
import type { ServiceConfig, ServiceRow, ServiceType, StartupMode } from "@/lib/types";
import { cn } from "@/lib/utils";

interface ServiceFormValues {
  serviceType: ServiceType;
  name: string;
  displayName: string;
  executable: string;
  arguments: string;
  workingDirectory: string;
  startup: StartupMode;
  restartDelaySec: string;
  dependencies: string;
  restartOnCrash: boolean;
  stdoutLog: string;
  stderrLog: string;
  apacheRoot: string;
  apacheConfig: string;
  pgData: string;
  pgAccount: string;
}

function emptyFormValues(): ServiceFormValues {
  return {
    serviceType: "nssm",
    name: "",
    displayName: "",
    executable: "",
    arguments: "",
    workingDirectory: "",
    startup: "auto",
    restartDelaySec: "5",
    dependencies: "",
    restartOnCrash: true,
    stdoutLog: "",
    stderrLog: "",
    apacheRoot: "",
    apacheConfig: "",
    pgData: "",
    pgAccount: "",
  };
}

function formValuesFromService(service: ServiceRow | null): ServiceFormValues {
  if (!service) return emptyFormValues();
  return {
    serviceType: service.serviceType,
    name: service.name,
    displayName: service.displayName,
    executable: service.executable,
    arguments: service.arguments,
    workingDirectory: service.workingDirectory,
    startup: service.startup,
    restartDelaySec: String(service.restartDelaySec),
    dependencies: (service.dependencies || []).join(", "),
    restartOnCrash: Boolean(service.restartOnCrash),
    stdoutLog: service.stdoutLog,
    stderrLog: service.stderrLog,
    apacheRoot: service.apacheRoot,
    apacheConfig: service.apacheConfig,
    pgData: service.pgData,
    pgAccount: service.pgAccount,
  };
}

interface FormFieldProps {
  label: string;
  htmlFor?: string;
  className?: string;
  children: ReactNode;
}

function FormField({ label, htmlFor, className, children }: FormFieldProps) {
  return (
    <div className={cn("space-y-1.5", className)}>
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
    </div>
  );
}

function SectionHeading({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center gap-2 pt-1 sm:col-span-2">
      <span className="text-muted-foreground">{icon}</span>
      <span className="text-sm font-medium">{children}</span>
      <div className="ml-1 h-px flex-1 bg-border" />
    </div>
  );
}

interface ServiceFormDialogProps {
  open: boolean;
  edit: boolean;
  service: ServiceRow | null;
  onOpenChange: (open: boolean) => void;
  onSaved: (rows: ServiceRow[], name: string, edited: boolean) => void;
}

export function ServiceFormDialog({
  open,
  edit,
  service,
  onOpenChange,
  onSaved,
}: ServiceFormDialogProps) {
  const [values, setValues] = useState<ServiceFormValues>(() => formValuesFromService(service));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open) {
      setValues(formValuesFromService(service));
      setError(null);
      setSaving(false);
    }
    // Reset only when the dialog opens or targets a different service. Background
    // refreshes replace the service object but must not overwrite user edits.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, edit, service?.name]);

  function update<K extends keyof ServiceFormValues>(key: K, value: ServiceFormValues[K]) {
    setValues((previous) => ({ ...previous, [key]: value }));
  }

  async function browseExecutable() {
    setError(null);
    try {
      const selected = await invoke<string | null>("pick_executable", {
        current: values.executable,
      });
      if (!selected) return;
      setValues((previous) => {
        const next = { ...previous, executable: selected };
        if (!previous.workingDirectory.trim()) {
          const separator = Math.max(selected.lastIndexOf("\\"), selected.lastIndexOf("/"));
          if (separator > 0) next.workingDirectory = selected.slice(0, separator);
        }
        if (!previous.displayName.trim()) {
          const fileName = selected.split(/[\\/]/).pop() || "";
          next.displayName = fileName.replace(/\.exe$/i, "");
        }
        return next;
      });
    } catch (browseError) {
      setError(errorMessage(browseError));
    }
  }

  async function browseWorkingDirectory() {
    setError(null);
    try {
      const selected = await invoke<string | null>("pick_directory", {
        current: values.workingDirectory,
      });
      if (selected) update("workingDirectory", selected);
    } catch (browseError) {
      setError(errorMessage(browseError));
    }
  }

  async function save() {
    const name = values.name.trim();
    const executable = values.executable.trim();
    if (!name) {
      setError("Service name is required.");
      return;
    }
    if (!executable) {
      setError("Application is required.");
      return;
    }

    const now = new Date().toISOString();
    const payload: ServiceConfig = {
      name,
      displayName: values.displayName.trim(),
      serviceType: values.serviceType,
      executable,
      arguments: values.arguments.trim(),
      workingDirectory: values.workingDirectory.trim(),
      apacheRoot: values.apacheRoot.trim(),
      apacheConfig: values.apacheConfig.trim(),
      pgData: values.pgData.trim(),
      pgAccount: values.pgAccount.trim(),
      stdoutLog: values.stdoutLog.trim(),
      stderrLog: values.stderrLog.trim(),
      startup: values.startup,
      restartOnCrash: values.restartOnCrash,
      restartDelaySec: Math.max(1, Number.parseInt(values.restartDelaySec, 10) || 5),
      dependencies: splitDependencies(values.dependencies),
      addedAt: service?.addedAt || now,
      updatedAt: now,
    };

    setSaving(true);
    setError(null);
    try {
      const rows = await invoke<ServiceRow[]>("save_service", { service: payload, edit });
      setSaving(false);
      onSaved(rows, name, edit);
      onOpenChange(false);
    } catch (saveError) {
      setError(errorMessage(saveError));
      setSaving(false);
    }
  }

  const isApache = values.serviceType === "apache";
  const isPostgres = values.serviceType === "postgresql";

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!saving) onOpenChange(nextOpen);
      }}
    >
      <DialogContent className="flex max-h-[92vh] w-full flex-col gap-0 overflow-hidden p-0 sm:max-w-[860px]">
        <DialogHeader className="border-b px-6 py-4 pr-14">
          <DialogTitle>{edit ? "Edit Service" : "Add Service"}</DialogTitle>
          <DialogDescription>
            {edit
              ? "Update the configuration for this service."
              : "Register a new service configuration."}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
          {error && (
            <div className="mb-4 flex gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span className="whitespace-pre-wrap">{error}</span>
            </div>
          )}

          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <FormField label="Type" htmlFor="service-type">
              <Select
                value={values.serviceType}
                onValueChange={(value) => update("serviceType", value as ServiceType)}
                disabled={saving}
              >
                <SelectTrigger id="service-type">
                  <SelectValue placeholder="Select a type" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="nssm">NSSM</SelectItem>
                  <SelectItem value="apache">Apache (native)</SelectItem>
                  <SelectItem value="postgresql">PostgreSQL (native)</SelectItem>
                </SelectContent>
              </Select>
            </FormField>

            <FormField label="Service name" htmlFor="service-name">
              <Input
                id="service-name"
                placeholder="my-app"
                value={values.name}
                disabled={edit || saving}
                onChange={(event) => update("name", event.target.value)}
              />
            </FormField>

            <FormField label="Display name" htmlFor="service-display-name" className="sm:col-span-2">
              <Input
                id="service-display-name"
                placeholder="My App"
                value={values.displayName}
                disabled={saving}
                onChange={(event) => update("displayName", event.target.value)}
              />
            </FormField>

            <FormField
              label="Application (.exe)"
              htmlFor="service-executable"
              className="sm:col-span-2"
            >
              <div className="flex items-center gap-2">
                <Input
                  id="service-executable"
                  placeholder="C:\apps\app.exe"
                  value={values.executable}
                  disabled={saving}
                  onChange={(event) => update("executable", event.target.value)}
                  className="flex-1"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-9 shrink-0"
                  title="Browse for executable"
                  onClick={() => void browseExecutable()}
                  disabled={saving}
                >
                  <FileSearch />
                  Browse…
                </Button>
              </div>
            </FormField>

            <FormField label="Arguments" htmlFor="service-arguments" className="sm:col-span-2">
              <Input
                id="service-arguments"
                placeholder="index.js --port 3000"
                value={values.arguments}
                disabled={saving}
                onChange={(event) => update("arguments", event.target.value)}
              />
            </FormField>

            <FormField
              label="Working directory"
              htmlFor="service-working-directory"
              className="sm:col-span-2"
            >
              <div className="flex items-center gap-2">
                <Input
                  id="service-working-directory"
                  placeholder="C:\apps"
                  value={values.workingDirectory}
                  disabled={saving}
                  onChange={(event) => update("workingDirectory", event.target.value)}
                  className="flex-1"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-9 shrink-0"
                  title="Browse for folder"
                  onClick={() => void browseWorkingDirectory()}
                  disabled={saving}
                >
                  <FolderOpen />
                  Browse…
                </Button>
              </div>
            </FormField>

            <FormField label="Startup" htmlFor="service-startup">
              <Select
                value={values.startup}
                onValueChange={(value) => update("startup", value as StartupMode)}
                disabled={saving}
              >
                <SelectTrigger id="service-startup">
                  <SelectValue placeholder="Select startup mode" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="auto">Automatic</SelectItem>
                  <SelectItem value="delayed">Automatic (Delayed)</SelectItem>
                  <SelectItem value="manual">Manual</SelectItem>
                </SelectContent>
              </Select>
            </FormField>

            <FormField label="Restart delay (seconds)" htmlFor="service-restart-delay">
              <Input
                id="service-restart-delay"
                type="number"
                min={1}
                step={1}
                value={values.restartDelaySec}
                disabled={saving}
                onChange={(event) => update("restartDelaySec", event.target.value)}
              />
            </FormField>

            <FormField
              label="Dependencies (space or comma separated)"
              htmlFor="service-dependencies"
              className="sm:col-span-2"
            >
              <Input
                id="service-dependencies"
                placeholder="Tcpip, postgresql-16"
                value={values.dependencies}
                disabled={saving}
                onChange={(event) => update("dependencies", event.target.value)}
              />
            </FormField>

            <div className="flex items-center justify-between gap-4 rounded-lg border bg-muted/30 px-4 py-3 sm:col-span-2">
              <div className="space-y-0.5">
                <Label htmlFor="service-restart-on-crash">Restart automatically on crash</Label>
                <p className="text-xs text-muted-foreground">
                  The restart delay is applied when the process exits unexpectedly.
                </p>
              </div>
              <Switch
                id="service-restart-on-crash"
                checked={values.restartOnCrash}
                onCheckedChange={(checked) => update("restartOnCrash", checked)}
                disabled={saving}
              />
            </div>

            <SectionHeading icon={<FileText className="h-4 w-4" />}>Logs</SectionHeading>

            <FormField label="Stdout log" htmlFor="service-stdout-log">
              <Input
                id="service-stdout-log"
                placeholder="C:\apps\logs\out.log"
                value={values.stdoutLog}
                disabled={saving}
                onChange={(event) => update("stdoutLog", event.target.value)}
              />
            </FormField>

            <FormField label="Stderr log" htmlFor="service-stderr-log">
              <Input
                id="service-stderr-log"
                placeholder="C:\apps\logs\err.log"
                value={values.stderrLog}
                disabled={saving}
                onChange={(event) => update("stderrLog", event.target.value)}
              />
            </FormField>

            {isApache && (
              <div className="space-y-3 rounded-lg border bg-muted/20 p-4 sm:col-span-2">
                <div className="flex items-center gap-2 text-sm font-medium">
                  <Server className="h-4 w-4 text-muted-foreground" />
                  Apache settings
                </div>
                <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                  <FormField label="Apache root" htmlFor="service-apache-root">
                    <Input
                      id="service-apache-root"
                      placeholder="C:\Apache24"
                      value={values.apacheRoot}
                      disabled={saving}
                      onChange={(event) => update("apacheRoot", event.target.value)}
                    />
                  </FormField>
                  <FormField label="Apache config" htmlFor="service-apache-config">
                    <Input
                      id="service-apache-config"
                      placeholder="conf/httpd.conf"
                      value={values.apacheConfig}
                      disabled={saving}
                      onChange={(event) => update("apacheConfig", event.target.value)}
                    />
                  </FormField>
                </div>
              </div>
            )}

            {isPostgres && (
              <div className="space-y-3 rounded-lg border bg-muted/20 p-4 sm:col-span-2">
                <div className="flex items-center gap-2 text-sm font-medium">
                  <Database className="h-4 w-4 text-muted-foreground" />
                  PostgreSQL settings
                </div>
                <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                  <FormField label="PGDATA" htmlFor="service-pg-data">
                    <Input
                      id="service-pg-data"
                      placeholder="C:\PostgreSQL\16\data"
                      value={values.pgData}
                      disabled={saving}
                      onChange={(event) => update("pgData", event.target.value)}
                    />
                  </FormField>
                  <FormField label="Service account" htmlFor="service-pg-account">
                    <Input
                      id="service-pg-account"
                      placeholder="NT AUTHORITY\NetworkService"
                      value={values.pgAccount}
                      disabled={saving}
                      onChange={(event) => update("pgAccount", event.target.value)}
                    />
                  </FormField>
                </div>
              </div>
            )}
          </div>
        </div>

        <DialogFooter className="flex-col gap-3 border-t bg-muted/20 px-6 py-4 sm:flex-row sm:items-center sm:justify-between sm:space-x-0">
          <p className="text-xs text-muted-foreground">
            {edit
              ? "The service name cannot be changed while editing."
              : "The service can be installed after the config is saved."}
          </p>
          <div className="flex w-full justify-end gap-2 sm:w-auto">
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
              disabled={saving}
            >
              Cancel
            </Button>
            <Button type="button" onClick={() => void save()} disabled={saving}>
              {saving ? <Loader2 className="animate-spin" /> : <Save />}
              Save
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
