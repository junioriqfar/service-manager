import { ExternalLink, FileText, RefreshCw } from "lucide-react";
import type { RefObject } from "react";

import { TooltipText } from "@/components/tooltip-text";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import type { LogView } from "@/lib/types";

interface LogPanelProps {
  serviceName: string | null;
  logView: LogView | null;
  autoRefresh: boolean;
  onAutoRefreshChange: (enabled: boolean) => void;
  onReload: () => void;
  onOpenFile: () => void;
  outputRef: RefObject<HTMLPreElement>;
}

export function LogPanel({
  serviceName,
  logView,
  autoRefresh,
  onAutoRefreshChange,
  onReload,
  onOpenFile,
  outputRef,
}: LogPanelProps) {
  const title = serviceName ? `Service log: ${serviceName}` : "Service log: (none selected)";

  return (
    <Card className="flex min-h-0 flex-[2] flex-col overflow-hidden shadow-sm">
      <CardHeader className="flex flex-row flex-wrap items-center gap-x-3 gap-y-2 space-y-0 border-b bg-muted/30 px-4 py-2.5">
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="h-4 w-4 shrink-0 text-muted-foreground" />
          <CardTitle className="truncate text-sm font-semibold">{title}</CardTitle>
        </div>
        <TooltipText
          text={logView?.path ?? ""}
          className="min-w-0 flex-1 text-xs text-muted-foreground"
        >
          {logView?.path ?? ""}
        </TooltipText>
        <div className="ml-auto flex flex-wrap items-center gap-3">
          <div className="flex items-center gap-2">
            <Switch
              id="log-auto-refresh"
              checked={autoRefresh}
              onCheckedChange={onAutoRefreshChange}
            />
            <Label
              htmlFor="log-auto-refresh"
              className="cursor-pointer text-xs font-normal text-muted-foreground"
            >
              Auto refresh
            </Label>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={onReload}
            disabled={!serviceName}
          >
            <RefreshCw />
            Reload
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={onOpenFile}
            disabled={!serviceName}
          >
            <ExternalLink />
            Open file
          </Button>
        </div>
      </CardHeader>
      <CardContent className="relative min-h-0 flex-1 p-0">
        <pre
          ref={outputRef}
          className="h-full w-full overflow-auto whitespace-pre bg-zinc-950 px-4 py-3 font-mono text-xs leading-relaxed text-zinc-200"
        >
          {logView?.text ?? ""}
        </pre>
        {!serviceName && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center text-xs text-zinc-500">
            Select a service to view its log.
          </div>
        )}
      </CardContent>
    </Card>
  );
}
