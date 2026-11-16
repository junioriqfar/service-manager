import { Download, Play, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { ServiceRow } from "@/lib/types";

interface DeleteServiceDialogProps {
  service: ServiceRow | null;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}

export function DeleteServiceDialog({
  service,
  onOpenChange,
  onConfirm,
}: DeleteServiceDialogProps) {
  return (
    <Dialog open={Boolean(service)} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Trash2 className="h-5 w-5 text-destructive" />
            Delete service
          </DialogTitle>
          <DialogDescription>
            Delete service &apos;{service?.name ?? ""}&apos; from Windows and the config?
          </DialogDescription>
        </DialogHeader>
        <div className="rounded-md border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">
          This removes the Windows service and its entry in the config. This cannot be undone.
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="button" variant="destructive" onClick={onConfirm}>
            <Trash2 />
            Delete
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

interface InstallServiceDialogProps {
  service: ServiceRow | null;
  onOpenChange: (open: boolean) => void;
  onConfirm: (startAfter: boolean) => void;
}

export function InstallServiceDialog({
  service,
  onOpenChange,
  onConfirm,
}: InstallServiceDialogProps) {
  return (
    <Dialog open={Boolean(service)} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Download className="h-5 w-5 text-primary" />
            Install service
          </DialogTitle>
          <DialogDescription>
            Install service &apos;{service?.name ?? ""}&apos; now?
          </DialogDescription>
        </DialogHeader>
        <div className="flex gap-2 rounded-md border bg-muted/30 p-3 text-sm text-muted-foreground">
          <Play className="mt-0.5 h-4 w-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
          <span>Choose whether to start the service immediately after installation.</span>
        </div>
        <DialogFooter className="flex-wrap gap-2 sm:space-x-0">
          <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="button" variant="outline" onClick={() => onConfirm(false)}>
            <Download />
            Install only
          </Button>
          <Button type="button" onClick={() => onConfirm(true)}>
            <Play />
            Install &amp; Start
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
