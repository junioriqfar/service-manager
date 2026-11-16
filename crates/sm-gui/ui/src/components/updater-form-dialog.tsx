import { useEffect, useState, type ReactNode } from "react";
import { FolderOpen, Loader2, Save } from "lucide-react";
import { toast } from "sonner";

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
import { errorMessage, invoke } from "@/lib/tauri";
import type { ServiceRow, UpdaterApp, UpdaterAppInput } from "@/lib/types";
import { cn } from "@/lib/utils";

const PRESETS = ["auto", "laravel", "node", "bun", "go", "static", "custom"] as const;
const PACKAGE_MANAGERS = ["auto", "npm", "pnpm", "yarn", "bun", "none"] as const;
const TOGGLES = ["auto", "always", "never"] as const;

interface UpdaterFormValues {
  id: string;
  name: string;
  enabled: boolean;
  repoUrl: string;
  localPath: string;
  branch: string;
  preset: string;
  packageManager: string;
  composer: string;
  artisan: string;
  packageInstall: string;
  build: string;
  installCommand: string;
  buildCommand: string;
  queueRestart: boolean;
  cloneOnFirstRun: boolean;
  allowedClientIPs: string;
  webServerService: string;
  stopBeforePull: string;
  stopBeforeBuild: string;
  restartAfter: string;
  startAfter: string;
  waitTimeoutSeconds: string;
  healthCheckEnabled: boolean;
  healthCheckUrl: string;
  healthCheckTimeout: string;
  notifyWebhookUrl: string;
  notifyOn: string;
}

function emptyValues(): UpdaterFormValues {
  return {
    id: "",
    name: "",
    enabled: true,
    repoUrl: "",
    localPath: "",
    branch: "main",
    preset: "auto",
    packageManager: "auto",
    composer: "auto",
    artisan: "auto",
    packageInstall: "auto",
    build: "auto",
    installCommand: "",
    buildCommand: "",
    queueRestart: true,
    cloneOnFirstRun: false,
    allowedClientIPs: "",
    webServerService: "",
    stopBeforePull: "",
    stopBeforeBuild: "",
    restartAfter: "",
    startAfter: "",
    waitTimeoutSeconds: "60",
    healthCheckEnabled: false,
    healthCheckUrl: "",
    healthCheckTimeout: "10",
    notifyWebhookUrl: "",
    notifyOn: "failed, degraded, updated",
  };
}

function valuesFromApp(app: UpdaterApp | null): UpdaterFormValues {
  if (!app) return emptyValues();
  return {
    id: app.id,
    name: app.name,
    enabled: app.enabled,
    repoUrl: app.repoUrl,
    localPath: app.localPath,
    branch: app.branch,
    preset: app.preset,
    packageManager: app.packageManager,
    composer: app.composer,
    artisan: app.artisan,
    packageInstall: app.packageInstall,
    build: app.build,
    installCommand: app.installCommand,
    buildCommand: app.buildCommand,
    queueRestart: app.queueRestart,
    cloneOnFirstRun: app.cloneOnFirstRun,
    allowedClientIPs: app.allowedClientIPs.join(", "),
    webServerService: app.webServerService,
    stopBeforePull: app.services.stopBeforePull.join(", "),
    stopBeforeBuild: app.services.stopBeforeBuild.join(", "),
    restartAfter: app.services.restartAfter.join(", "),
    startAfter: app.services.startAfter.join(", "),
    waitTimeoutSeconds: String(app.services.waitTimeoutSeconds || 60),
    healthCheckEnabled: app.healthCheck.enabled,
    healthCheckUrl: app.healthCheck.url,
    healthCheckTimeout: String(app.healthCheck.timeoutSeconds || 10),
    notifyWebhookUrl: app.notify.webhookUrl,
    notifyOn: app.notify.on.join(", "),
  };
}

function splitList(value: string): string[] {
  return value
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function toInput(values: UpdaterFormValues): UpdaterAppInput {
  return {
    id: values.id.trim(),
    name: values.name.trim() || values.id.trim(),
    enabled: values.enabled,
    repoUrl: values.repoUrl.trim(),
    localPath: values.localPath.trim(),
    branch: values.branch.trim() || "main",
    preset: values.preset,
    packageManager: values.packageManager,
    composer: values.composer,
    artisan: values.artisan,
    packageInstall: values.packageInstall,
    build: values.build,
    installCommand: values.installCommand.trim(),
    buildCommand: values.buildCommand.trim(),
    queueRestart: values.queueRestart,
    cloneOnFirstRun: values.cloneOnFirstRun,
    allowedClientIPs: splitList(values.allowedClientIPs),
    webServerService: values.webServerService.trim(),
    services: {
      stopBeforePull: splitList(values.stopBeforePull),
      stopBeforeBuild: splitList(values.stopBeforeBuild),
      restartAfter: splitList(values.restartAfter),
      startAfter: splitList(values.startAfter),
      waitTimeoutSeconds: Math.max(1, Number(values.waitTimeoutSeconds) || 60),
    },
    healthCheck: {
      enabled: values.healthCheckEnabled,
      url: values.healthCheckUrl.trim(),
      timeoutSeconds: Math.max(1, Number(values.healthCheckTimeout) || 10),
    },
    notify: {
      webhookUrl: values.notifyWebhookUrl.trim(),
      on: splitList(values.notifyOn),
    },
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

function SectionHeading({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-center gap-2 pt-1 sm:col-span-2">
      <span className="text-sm font-medium">{children}</span>
      <div className="ml-1 h-px flex-1 bg-border" />
    </div>
  );
}

interface SelectFieldProps {
  label: string;
  value: string;
  options: readonly string[];
  onChange: (value: string) => void;
}

function SelectField({ label, value, options, onChange }: SelectFieldProps) {
  const id = `updater-${label.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
  return (
    <FormField label={label} htmlFor={id}>
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger id={id}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map((option) => (
            <SelectItem key={option} value={option}>
              {option}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </FormField>
  );
}

interface UpdaterFormDialogProps {
  open: boolean;
  edit: boolean;
  app: UpdaterApp | null;
  onOpenChange: (open: boolean) => void;
  onSaved: (apps: UpdaterApp[], id: string, edited: boolean) => void;
}

export function UpdaterFormDialog({
  open,
  edit,
  app,
  onOpenChange,
  onSaved,
}: UpdaterFormDialogProps) {
  const [values, setValues] = useState<UpdaterFormValues>(() => valuesFromApp(app));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [apacheServices, setApacheServices] = useState<string[]>([]);

  useEffect(() => {
    if (open) {
      setValues(valuesFromApp(app));
      setError(null);
    }
  }, [open, app]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void (async () => {
      try {
        const rows = await invoke<ServiceRow[]>("list_services");
        if (cancelled) return;
        setApacheServices(
          (Array.isArray(rows) ? rows : [])
            .filter((row) => row.serviceType === "apache")
            .map((row) => row.name)
            .filter((name) => name.trim().length > 0)
            .sort((left, right) => left.localeCompare(right)),
        );
      } catch (caught) {
        if (!cancelled) toast.error(errorMessage(caught));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open]);

  function set<K extends keyof UpdaterFormValues>(key: K, value: UpdaterFormValues[K]) {
    setValues((previous) => ({ ...previous, [key]: value }));
  }

  async function browseFolder() {
    try {
      const selected = await invoke<string | null>("pick_directory", {
        current: values.localPath || null,
      });
      if (selected) set("localPath", selected);
    } catch (caught) {
      toast.error(errorMessage(caught));
    }
  }

  async function save() {
    if (saving) return;
    setError(null);
    if (!values.id.trim()) {
      setError("App ID is required.");
      return;
    }
    if (!values.repoUrl.trim()) {
      setError("Repository URL is required.");
      return;
    }
    if (!values.localPath.trim()) {
      setError("Local path is required.");
      return;
    }
    setSaving(true);
    try {
      const apps = await invoke<UpdaterApp[]>("save_updater_app", { app: toInput(values) });
      onSaved(apps, values.id.trim(), edit);
      onOpenChange(false);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setSaving(false);
    }
  }

  const apacheOptions = Array.from(
    new Set([
      ...apacheServices,
      ...(values.webServerService.trim() ? [values.webServerService.trim()] : []),
    ]),
  ).sort((left, right) => left.localeCompare(right));

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{edit ? "Edit Updater App" : "Add Updater App"}</DialogTitle>
          <DialogDescription>
            Registered applications are updated only when you press Run or Check.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 py-2 sm:grid-cols-2">
          <SectionHeading>Application</SectionHeading>
          <FormField label="App ID" htmlFor="updater-app-id">
            <Input
              id="updater-app-id"
              value={values.id}
              disabled={edit}
              onChange={(event) => set("id", event.target.value)}
              placeholder="bsi-qris-api"
            />
          </FormField>
          <FormField label="Display name" htmlFor="updater-app-name">
            <Input
              id="updater-app-name"
              value={values.name}
              onChange={(event) => set("name", event.target.value)}
              placeholder="BSI QRIS API"
            />
          </FormField>
          <FormField label="Repository URL" htmlFor="updater-app-repo" className="sm:col-span-2">
            <Input
              id="updater-app-repo"
              value={values.repoUrl}
              onChange={(event) => set("repoUrl", event.target.value)}
              placeholder="https://github.com/owner/repo.git"
            />
          </FormField>
          <FormField label="Local path" htmlFor="updater-app-path" className="sm:col-span-2">
            <div className="flex gap-2">
              <Input
                id="updater-app-path"
                value={values.localPath}
                onChange={(event) => set("localPath", event.target.value)}
                placeholder="D:\Projects\NodeProjects\app"
              />
              <Button type="button" variant="outline" onClick={() => void browseFolder()} title="Choose the project folder">
                <FolderOpen />
                Browse…
              </Button>
            </div>
          </FormField>
          <FormField label="Branch" htmlFor="updater-app-branch">
            <Input
              id="updater-app-branch"
              value={values.branch}
              onChange={(event) => set("branch", event.target.value)}
            />
          </FormField>
          <div className="flex items-center justify-between gap-4 rounded-md border px-3 py-2">
            <div>
              <Label htmlFor="updater-app-enabled">Enabled</Label>
              <p className="text-xs text-muted-foreground">Include this app in Run All.</p>
            </div>
            <Switch
              id="updater-app-enabled"
              checked={values.enabled}
              onCheckedChange={(checked) => set("enabled", checked)}
            />
          </div>

          <SectionHeading>Toolchain</SectionHeading>
          <SelectField
            label="Preset"
            value={values.preset}
            options={PRESETS}
            onChange={(value) => set("preset", value)}
          />
          <SelectField
            label="Package manager"
            value={values.packageManager}
            options={PACKAGE_MANAGERS}
            onChange={(value) => set("packageManager", value)}
          />
          <SelectField
            label="Composer"
            value={values.composer}
            options={TOGGLES}
            onChange={(value) => set("composer", value)}
          />
          <SelectField
            label="Artisan"
            value={values.artisan}
            options={TOGGLES}
            onChange={(value) => set("artisan", value)}
          />
          <SelectField
            label="Package install"
            value={values.packageInstall}
            options={TOGGLES}
            onChange={(value) => set("packageInstall", value)}
          />
          <SelectField
            label="Build"
            value={values.build}
            options={TOGGLES}
            onChange={(value) => set("build", value)}
          />
          <FormField label="Install command" htmlFor="updater-app-install" className="sm:col-span-2">
            <Input
              id="updater-app-install"
              value={values.installCommand}
              onChange={(event) => set("installCommand", event.target.value)}
              placeholder='bun install --frozen-lockfile'
            />
          </FormField>
          <FormField label="Build command" htmlFor="updater-app-build" className="sm:col-span-2">
            <Input
              id="updater-app-build"
              value={values.buildCommand}
              onChange={(event) => set("buildCommand", event.target.value)}
              placeholder="bun run build:win"
            />
          </FormField>
          <div className="flex items-center justify-between gap-4 rounded-md border px-3 py-2 sm:col-span-2">
            <div>
              <Label htmlFor="updater-app-queue">Queue restart</Label>
              <p className="text-xs text-muted-foreground">
                Run php artisan queue:restart after Laravel code changes.
              </p>
            </div>
            <Switch
              id="updater-app-queue"
              checked={values.queueRestart}
              onCheckedChange={(checked) => set("queueRestart", checked)}
            />
          </div>

          <SectionHeading>Service lifecycle</SectionHeading>
          <FormField label="Stop before pull" htmlFor="updater-stop-pull">
            <Input
              id="updater-stop-pull"
              value={values.stopBeforePull}
              onChange={(event) => set("stopBeforePull", event.target.value)}
              placeholder="service-a, service-b"
            />
          </FormField>
          <FormField label="Stop before build" htmlFor="updater-stop-build">
            <Input
              id="updater-stop-build"
              value={values.stopBeforeBuild}
              onChange={(event) => set("stopBeforeBuild", event.target.value)}
              placeholder="opencode-quota"
            />
          </FormField>
          <FormField label="Restart after update" htmlFor="updater-restart">
            <Input
              id="updater-restart"
              value={values.restartAfter}
              onChange={(event) => set("restartAfter", event.target.value)}
              placeholder="bsi-qris-api"
            />
          </FormField>
          <FormField label="Start after update" htmlFor="updater-start">
            <Input
              id="updater-start"
              value={values.startAfter}
              onChange={(event) => set("startAfter", event.target.value)}
              placeholder="optional"
            />
          </FormField>
          <FormField label="Wait timeout (seconds)" htmlFor="updater-wait">
            <Input
              id="updater-wait"
              type="number"
              min={1}
              value={values.waitTimeoutSeconds}
              onChange={(event) => set("waitTimeoutSeconds", event.target.value)}
            />
          </FormField>
          <FormField
            label="Web server (Apache)"
            htmlFor="updater-web-server"
            className="sm:col-span-2"
          >
            <Select
              value={values.webServerService || "__none__"}
              onValueChange={(value) =>
                set("webServerService", value === "__none__" ? "" : value)
              }
            >
              <SelectTrigger
                id="updater-web-server"
                title="Optional Apache service from data/services.json that serves this app."
              >
                <SelectValue placeholder="Not linked" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">Not linked</SelectItem>
                {apacheOptions.map((name) => (
                  <SelectItem key={name} value={name}>
                    {name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">
              Only services with serviceType apache are listed. Leave empty when the app is
              not served by Apache.
            </p>
          </FormField>

          <SectionHeading>Access</SectionHeading>
          <FormField
            label="Per-app allowed IPs / CIDR"
            htmlFor="updater-app-ips"
            className="sm:col-span-2"
          >
            <Input
              id="updater-app-ips"
              value={values.allowedClientIPs}
              onChange={(event) => set("allowedClientIPs", event.target.value)}
              placeholder="10.255.240.143, 10.255.240.0/24 (empty = global allowlist)"
            />
          </FormField>

          <SectionHeading>Clone, health, notification</SectionHeading>
          <div className="flex items-center justify-between gap-4 rounded-md border px-3 py-2 sm:col-span-2">
            <div>
              <Label htmlFor="updater-app-clone">Clone on first run</Label>
              <p className="text-xs text-muted-foreground">
                Run git clone when the local path does not exist.
              </p>
            </div>
            <Switch
              id="updater-app-clone"
              checked={values.cloneOnFirstRun}
              onCheckedChange={(checked) => set("cloneOnFirstRun", checked)}
            />
          </div>
          <div className="flex items-center justify-between gap-4 rounded-md border px-3 py-2 sm:col-span-2">
            <div>
              <Label htmlFor="updater-app-health">Health check</Label>
              <p className="text-xs text-muted-foreground">
                HTTP check after services restart; failure marks the run degraded.
              </p>
            </div>
            <Switch
              id="updater-app-health"
              checked={values.healthCheckEnabled}
              onCheckedChange={(checked) => set("healthCheckEnabled", checked)}
            />
          </div>
          <FormField label="Health check URL" htmlFor="updater-health-url">
            <Input
              id="updater-health-url"
              value={values.healthCheckUrl}
              onChange={(event) => set("healthCheckUrl", event.target.value)}
              placeholder="http://127.0.0.1:8100/health"
            />
          </FormField>
          <FormField label="Health timeout (seconds)" htmlFor="updater-health-timeout">
            <Input
              id="updater-health-timeout"
              type="number"
              min={1}
              value={values.healthCheckTimeout}
              onChange={(event) => set("healthCheckTimeout", event.target.value)}
            />
          </FormField>
          <FormField label="Webhook URL" htmlFor="updater-notify-url" className="sm:col-span-2">
            <Input
              id="updater-notify-url"
              value={values.notifyWebhookUrl}
              onChange={(event) => set("notifyWebhookUrl", event.target.value)}
              placeholder="https://hooks.example.com/updater"
            />
          </FormField>
          <FormField label="Notify on" htmlFor="updater-notify-on" className="sm:col-span-2">
            <Input
              id="updater-notify-on"
              value={values.notifyOn}
              onChange={(event) => set("notifyOn", event.target.value)}
              placeholder="failed, degraded, updated"
            />
          </FormField>
        </div>

        {error && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}

        <DialogFooter>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)} title="Discard changes">
            Cancel
          </Button>
          <Button type="button" onClick={() => void save()} disabled={saving} title="Save this app to the registry">
            {saving ? <Loader2 className="animate-spin" /> : <Save />}
            {saving ? "Saving…" : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
