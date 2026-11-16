import {
  Download,
  FileText,
  Import as ImportIcon,
  ListChecks,
  Loader2,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  RotateCcw,
  ServerOff,
  Square,
  Trash2,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  countsLabel,
  restartLabel,
  startupLabel,
  statusVariant,
  typeLabel,
} from "@/lib/service";
import type { ServiceAction, ServiceRow } from "@/lib/types";
import { cn } from "@/lib/utils";

interface ServiceTableProps {
  services: ServiceRow[];
  selectedName: string | null;
  batchRunning: boolean;
  pendingService: string | null;
  refreshing: boolean;
  importing: boolean;
  onSelect: (name: string) => void;
  onAdd: () => void;
  onImport: () => void;
  onRefresh: () => void;
  onEdit: (service: ServiceRow) => void;
  onAction: (action: ServiceAction, service: ServiceRow) => void;
}

export function ServiceTable({
  services,
  selectedName,
  batchRunning,
  pendingService,
  refreshing,
  importing,
  onSelect,
  onAdd,
  onImport,
  onRefresh,
  onEdit,
  onAction,
}: ServiceTableProps) {
  return (
    <Card className="flex min-h-0 flex-[3] flex-col overflow-hidden shadow-sm">
      <CardHeader className="flex flex-row flex-wrap items-center gap-x-3 gap-y-2 space-y-0 border-b bg-muted/30 px-4 py-2.5">
        <div className="flex items-center gap-2">
          <ListChecks className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-sm font-semibold">Service List</CardTitle>
        </div>
        <span className="text-xs text-muted-foreground">{countsLabel(services)}</span>
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <span className="hidden text-xs text-muted-foreground min-[1100px]:inline">
            Right-click a service for actions
          </span>
          <Button type="button" variant="outline" size="sm" onClick={onAdd}>
            <Plus />
            Add
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={onImport}
            disabled={importing}
          >
            {importing ? <Loader2 className="animate-spin" /> : <ImportIcon />}
            Import
          </Button>
          <Button type="button" variant="outline" size="sm" onClick={onRefresh}>
            <RefreshCw className={cn(refreshing && "animate-spin")} />
            Refresh
          </Button>
        </div>
      </CardHeader>
      <CardContent className="min-h-0 flex-1 p-0">
        <Table containerClassName="h-full overflow-auto" className="min-w-[820px] table-fixed">
          <TableHeader className="sticky top-0 z-10 bg-muted">
            <TableRow className="hover:bg-transparent">
              <TableHead className="w-[112px] text-xs font-semibold uppercase tracking-wide">
                Status
              </TableHead>
              <TableHead className="w-[140px] text-xs font-semibold uppercase tracking-wide">
                Service Name
              </TableHead>
              <TableHead className="w-[180px] text-xs font-semibold uppercase tracking-wide">
                Display Name
              </TableHead>
              <TableHead className="w-[92px] text-xs font-semibold uppercase tracking-wide">
                Type
              </TableHead>
              <TableHead className="text-xs font-semibold uppercase tracking-wide">
                Application
              </TableHead>
              <TableHead className="w-[120px] text-xs font-semibold uppercase tracking-wide">
                Startup
              </TableHead>
              <TableHead className="w-[84px] text-xs font-semibold uppercase tracking-wide">
                Restart
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {services.length === 0 ? (
              <TableRow className="hover:bg-transparent">
                <TableCell colSpan={7} className="h-40 text-center">
                  <div className="mx-auto flex max-w-md flex-col items-center gap-2 text-muted-foreground">
                    <ServerOff className="h-8 w-8 opacity-60" />
                    <p className="text-sm">
                      The list is empty. Click <strong className="text-foreground">Import</strong>{" "}
                      to scan installed services, or{" "}
                      <strong className="text-foreground">Add</strong> to create a new config.
                    </p>
                  </div>
                </TableCell>
              </TableRow>
            ) : (
              services.map((service) => {
                const busy = batchRunning || pendingService === service.name;
                const running = service.state === "running";
                const selected = selectedName === service.name;

                return (
                  <ContextMenu key={service.name}>
                    <ContextMenuTrigger asChild>
                      <TableRow
                        data-name={service.name}
                        className={cn(
                          "cursor-default",
                          selected && "bg-primary/10 hover:bg-primary/15",
                        )}
                        onClick={() => onSelect(service.name)}
                        onDoubleClick={() => onEdit(service)}
                        onContextMenu={() => onSelect(service.name)}
                      >
                        <TableCell>
                          <Badge
                            variant={statusVariant(service.state)}
                            className="max-w-full whitespace-nowrap px-2 py-0.5 text-[11px]"
                            title={service.error ?? service.stateLabel}
                          >
                            {pendingService === service.name && (
                              <Loader2 className="h-3 w-3 animate-spin" />
                            )}
                            {service.stateLabel}
                          </Badge>
                        </TableCell>
                        <TableCell className="truncate font-medium" title={service.name}>
                          {service.name}
                        </TableCell>
                        <TableCell
                          className="truncate text-muted-foreground"
                          title={service.displayName}
                        >
                          {service.displayName || "—"}
                        </TableCell>
                        <TableCell className="text-xs text-muted-foreground">
                          {typeLabel(service.serviceType)}
                        </TableCell>
                        <TableCell
                          className="truncate font-mono text-xs text-muted-foreground"
                          title={service.executable}
                        >
                          {service.executable || "—"}
                        </TableCell>
                        <TableCell className="text-xs text-muted-foreground">
                          {startupLabel(service.startup)}
                        </TableCell>
                        <TableCell className="text-xs text-muted-foreground">
                          {restartLabel(service)}
                        </TableCell>
                      </TableRow>
                    </ContextMenuTrigger>
                    <ContextMenuContent className="w-44">
                      <ContextMenuItem disabled={busy} onSelect={() => onEdit(service)}>
                        <Pencil />
                        Edit
                      </ContextMenuItem>
                      <ContextMenuItem
                        disabled={service.installed || busy}
                        onSelect={() => onAction("install", service)}
                      >
                        <Download />
                        Install
                      </ContextMenuItem>
                      <ContextMenuSeparator />
                      <ContextMenuItem
                        disabled={!service.installed || running || busy}
                        onSelect={() => onAction("start", service)}
                      >
                        <Play />
                        Start
                      </ContextMenuItem>
                      <ContextMenuItem
                        disabled={!service.installed || !running || busy}
                        onSelect={() => onAction("stop", service)}
                      >
                        <Square />
                        Stop
                      </ContextMenuItem>
                      <ContextMenuItem
                        disabled={!service.installed || busy}
                        onSelect={() => onAction("restart", service)}
                      >
                        <RotateCcw />
                        Restart
                      </ContextMenuItem>
                      <ContextMenuSeparator />
                      <ContextMenuItem onSelect={() => onAction("log", service)}>
                        <FileText />
                        Open Log
                      </ContextMenuItem>
                      <ContextMenuSeparator />
                      <ContextMenuItem
                        disabled={busy}
                        className="text-destructive focus:bg-destructive/10 focus:text-destructive"
                        onSelect={() => onAction("delete", service)}
                      >
                        <Trash2 />
                        Delete
                      </ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenu>
                );
              })
            )}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
}
