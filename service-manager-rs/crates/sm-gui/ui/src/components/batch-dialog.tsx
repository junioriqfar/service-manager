import {
  Ban,
  CheckCircle2,
  Loader2,
  X,
  XCircle,
  type LucideIcon,
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
import { Progress } from "@/components/ui/progress";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { BatchView } from "@/lib/types";

interface BatchDialogProps {
  batch: BatchView | null;
  running: boolean;
  onCancel: () => void;
  onClose: () => void;
}

function lineIcon(state: string): LucideIcon {
  if (state === "succeeded") return CheckCircle2;
  if (state === "cancelled") return Ban;
  return XCircle;
}

function lineIconClass(state: string): string {
  if (state === "succeeded") return "text-emerald-400";
  if (state === "cancelled") return "text-amber-400";
  return "text-red-400";
}

export function BatchDialog({ batch, running, onCancel, onClose }: BatchDialogProps) {
  const total = batch?.total ?? 0;
  const done = batch?.done ?? 0;
  const lines = batch?.lines ?? [];
  const failed = batch?.summary?.failed ?? 0;
  const cancelled = batch?.summary?.cancelled ?? 0;
  const progressValue = total > 0 ? Math.min(100, (done / total) * 100) : 0;

  const TitleIcon = running
    ? Loader2
    : failed > 0
      ? XCircle
      : cancelled > 0
        ? Ban
        : CheckCircle2;

  const titleIconClass = running
    ? "animate-spin text-primary"
    : failed > 0
      ? "text-destructive"
      : cancelled > 0
        ? "text-amber-500"
        : "text-emerald-500";

  return (
    <Dialog
      open={batch?.open ?? false}
      onOpenChange={(open) => {
        if (!open && !running) onClose();
      }}
    >
      <DialogContent
        hideClose
        className="sm:max-w-lg"
        onEscapeKeyDown={(event) => {
          if (running) event.preventDefault();
        }}
        onInteractOutside={(event) => {
          if (running) event.preventDefault();
        }}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 pr-2">
            <TitleIcon className={`h-4 w-4 shrink-0 ${titleIconClass}`} />
            Running {batch?.action ?? ""} for all services...
          </DialogTitle>
          <DialogDescription>Batch operation progress for all installed services.</DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          <div className="space-y-1.5">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>{running ? "In progress" : "Finished"}</span>
              <span>
                {done} / {total}
              </span>
            </div>
            <Progress value={progressValue} />
          </div>

          <p className="min-h-5 text-sm text-muted-foreground">{batch?.text ?? ""}</p>

          <ScrollArea className="h-48 rounded-md border border-zinc-800 bg-zinc-950">
            <div className="space-y-1.5 p-3 font-mono text-xs text-zinc-200">
              {lines.length === 0 ? (
                <p className="text-zinc-500">Preparing...</p>
              ) : (
                lines.map((line, index) => {
                  const Icon = lineIcon(line.state);
                  return (
                    <div key={`${line.name}-${index}`} className="flex items-start gap-2">
                      <Icon className={`mt-0.5 h-3.5 w-3.5 shrink-0 ${lineIconClass(line.state)}`} />
                      <span className="min-w-0 break-words">
                        <span className="font-medium text-zinc-100">{line.name}</span>
                        {line.message ? (
                          <span className="text-zinc-400"> - {line.message}</span>
                        ) : null}
                      </span>
                    </div>
                  );
                })
              )}
            </div>
          </ScrollArea>
        </div>

        <DialogFooter className="gap-2 sm:space-x-0">
          {running ? (
            <Button
              type="button"
              variant="outline"
              className="text-destructive hover:bg-destructive/10 hover:text-destructive"
              onClick={onCancel}
              disabled={batch?.cancelling}
            >
              {batch?.cancelling ? <Loader2 className="animate-spin" /> : <Ban />}
              Cancel
            </Button>
          ) : (
            <Button type="button" onClick={onClose}>
              <X />
              Close
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
