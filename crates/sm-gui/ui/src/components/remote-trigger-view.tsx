import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  Eye,
  Loader2,
  Play,
  RadioTower,
  Search,
  Terminal,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { errorMessage, invoke, listen } from "@/lib/tauri";
import type {
  RemoteTriggerConfig,
  RemoteTriggerOutput,
  RemoteTriggerResult,
} from "@/lib/types";
import { cn } from "@/lib/utils";

const DEFAULT_CONFIG: RemoteTriggerConfig = {
  host: "",
  user: "",
  port: 22,
  identity: "",
  wrapper: "C:\\tools\\ServiceManager\\scripts\\Invoke-UpdaterTask.ps1",
  remoteBin: "C:\\tools\\ServiceManager\\target\\release\\sm-updater.exe",
  remoteConfig: "",
  app: "",
};

interface OutputLine {
  stream: string;
  line: string;
}

function Field({
  label,
  htmlFor,
  className,
  children,
}: {
  label: string;
  htmlFor: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={cn("space-y-1.5", className)}>
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
    </div>
  );
}

export function RemoteTriggerView() {
  const [config, setConfig] = useState<RemoteTriggerConfig>(DEFAULT_CONFIG);
  const [output, setOutput] = useState<OutputLine[]>([]);
  const [running, setRunning] = useState(false);
  const [loading, setLoading] = useState(true);
  const [status, setStatus] = useState("Ready");
  const outputRef = useRef<HTMLDivElement>(null);
  const receivedRef = useRef(false);

  const append = useCallback((stream: string, line: string) => {
    setOutput((previous) => [...previous, { stream, line }]);
  }, []);

  useEffect(() => {
    let active = true;
    invoke<RemoteTriggerConfig>("load_remote_trigger")
      .then((value) => {
        if (active) setConfig({ ...DEFAULT_CONFIG, ...value });
      })
      .catch((error) => toast.error(errorMessage(error)))
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<RemoteTriggerOutput>("remote-trigger-output", (event) => {
      receivedRef.current = true;
      setOutput((previous) => [...previous, event.payload]);
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((error) => console.error("Failed to listen for trigger output.", error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const node = outputRef.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [output]);

  function set<K extends keyof RemoteTriggerConfig>(key: K, value: RemoteTriggerConfig[K]) {
    setConfig((previous) => ({ ...previous, [key]: value }));
  }

  async function save(showToast = true): Promise<RemoteTriggerConfig | null> {
    try {
      const saved = await invoke<RemoteTriggerConfig>("save_remote_trigger", { config });
      setConfig(saved);
      if (showToast) toast.success("Remote trigger settings saved.");
      return saved;
    } catch (error) {
      toast.error(errorMessage(error));
      return null;
    }
  }

  async function preview() {
    try {
      const command = await invoke<string>("preview_remote_trigger", {
        config,
        app: config.app.trim() || null,
        check: false,
      });
      append("command", `$ ${command}`);
      setStatus("Preview");
    } catch (error) {
      toast.error(errorMessage(error));
    }
  }

  async function testSsh() {
    if (running) return;
    setRunning(true);
    setStatus("Testing SSH...");
    append("command", "$ test ssh connection");
    try {
      const result = await invoke<RemoteTriggerResult>("test_remote_ssh", { config });
      append("command", `$ ${result.command}`);
      for (const line of result.output.split(/\r?\n/)) {
        if (line.length > 0) append("stdout", line);
      }
      setStatus(`SSH test exit ${result.exitCode}`);
      if (result.exitCode === 0) toast.success("SSH connection OK.");
      else toast.error(`SSH test failed (exit ${result.exitCode}).`);
    } catch (error) {
      append("stderr", errorMessage(error));
      setStatus("SSH test failed");
      toast.error(errorMessage(error));
    } finally {
      setRunning(false);
    }
  }

  async function run(appId: string | null, check: boolean) {
    if (running) return;
    const saved = await save(false);
    if (!saved) return;
    setRunning(true);
    setOutput([]);
    receivedRef.current = false;
    setStatus(check ? "Checking..." : "Updating...");
    append("command", check ? "=== Check trigger ===" : "=== Update trigger ===");
    try {
      const result = await invoke<RemoteTriggerResult>("run_remote_trigger", {
        config: saved,
        appId,
        check,
      });
      if (result.output && !receivedRef.current) {
        for (const line of result.output.split(/\r?\n/)) {
          if (line.length > 0) append("stdout", line);
        }
      }
      setStatus(`Finished with exit ${result.exitCode}`);
      if (result.exitCode === 0) toast.success("Remote trigger finished.");
      else toast.error(`Remote trigger exited with ${result.exitCode}.`);
    } catch (error) {
      append("stderr", errorMessage(error));
      setStatus("Remote trigger failed");
      toast.error(errorMessage(error), { duration: 10000 });
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="rounded-lg border bg-card px-3 py-2">
        <div className="flex flex-wrap items-center gap-2">
          <div className="mr-auto">
            <h2 className="flex items-center gap-2 text-sm font-semibold">
              <RadioTower className="h-4 w-4" />
              Remote Trigger
            </h2>
            <p className="text-xs text-muted-foreground">
              Run sm-trigger over SSH from this client. Key/agent auth only (BatchMode).
            </p>
          </div>
          <Badge variant={running ? "warning" : "secondary"}>{status}</Badge>
          <Button type="button" variant="outline" size="sm" onClick={() => void preview()} disabled={running}>
            <Eye />
            Preview
          </Button>
          <Button type="button" variant="outline" size="sm" onClick={() => void testSsh()} disabled={running}>
            <Terminal />
            Test SSH
          </Button>
          <Button type="button" size="sm" onClick={() => void save()} disabled={running || loading}>
            Save
          </Button>
        </div>

        <div className="mt-3 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          <Field label="Host" htmlFor="remote-host">
            <Input
              id="remote-host"
              value={config.host}
              onChange={(event) => set("host", event.target.value)}
              placeholder="10.255.240.160"
            />
          </Field>
          <Field label="User" htmlFor="remote-user">
            <Input
              id="remote-user"
              value={config.user}
              onChange={(event) => set("user", event.target.value)}
              placeholder="EGOV22"
            />
          </Field>
          <Field label="Port" htmlFor="remote-port">
            <Input
              id="remote-port"
              type="number"
              min={1}
              max={65535}
              value={config.port}
              onChange={(event) => set("port", Number(event.target.value) || 22)}
            />
          </Field>
          <Field label="Identity file" htmlFor="remote-identity">
            <Input
              id="remote-identity"
              value={config.identity}
              onChange={(event) => set("identity", event.target.value)}
              placeholder="C:\Users\you\.ssh\id_ed25519"
            />
          </Field>
          <Field label="Wrapper script" htmlFor="remote-wrapper" className="sm:col-span-2">
            <Input
              id="remote-wrapper"
              value={config.wrapper}
              onChange={(event) => set("wrapper", event.target.value)}
              placeholder="C:\tools\ServiceManager\scripts\Invoke-UpdaterTask.ps1"
            />
          </Field>
          <Field label="Remote sm-updater.exe" htmlFor="remote-bin" className="sm:col-span-2">
            <Input
              id="remote-bin"
              value={config.remoteBin}
              onChange={(event) => set("remoteBin", event.target.value)}
              placeholder="C:\tools\ServiceManager\target\release\sm-updater.exe"
            />
          </Field>
          <Field label="Remote projects.json (optional)" htmlFor="remote-config" className="sm:col-span-2">
            <Input
              id="remote-config"
              value={config.remoteConfig}
              onChange={(event) => set("remoteConfig", event.target.value)}
              placeholder="C:\tools\ServiceManager\data\projects.json"
            />
          </Field>
          <Field label="App ID (optional)" htmlFor="remote-app">
            <Input
              id="remote-app"
              value={config.app}
              onChange={(event) => set("app", event.target.value)}
              placeholder="opencode-quota"
            />
          </Field>
          <div className="flex items-end gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void run(config.app.trim() || null, true)}
              disabled={running || !config.app.trim()}
            >
              <Search />
              Check App
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={() => void run(config.app.trim() || null, false)}
              disabled={running || !config.app.trim()}
            >
              <Play />
              Run App
            </Button>
          </div>
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-2 border-t pt-3">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void run(null, true)}
            disabled={running || loading}
          >
            {running ? <Loader2 className="animate-spin" /> : <Search />}
            Check All
          </Button>
          <Button
            type="button"
            size="sm"
            onClick={() => void run(null, false)}
            disabled={running || loading}
          >
            {running ? <Loader2 className="animate-spin" /> : <Play />}
            Update All
          </Button>
          <span className="text-xs text-muted-foreground">
            Update All can stop/restart services on the server.
          </span>
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col rounded-lg border bg-card">
        <div className="flex items-center justify-between gap-2 border-b px-3 py-2">
          <div>
            <div className="text-sm font-medium">Remote output</div>
            <div className="text-xs text-muted-foreground">
              Streamed from ssh stdout/stderr. {output.length} line(s).
            </div>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setOutput([])}
            disabled={running || output.length === 0}
          >
            <Trash2 />
            Clear
          </Button>
        </div>
        <div
          ref={outputRef}
          className="min-h-24 flex-1 overflow-auto whitespace-pre-wrap break-words p-3 font-mono text-xs"
        >
          {output.length === 0 ? (
            <span className="text-muted-foreground">
              No output yet. Preview the command or run a trigger.
            </span>
          ) : (
            output.map((item, index) => (
              <div
                key={`${index}-${item.line}`}
                className={cn(
                  item.stream === "stderr" && "text-amber-600 dark:text-amber-400",
                  item.stream === "command" && "text-muted-foreground",
                )}
              >
                {item.line}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
