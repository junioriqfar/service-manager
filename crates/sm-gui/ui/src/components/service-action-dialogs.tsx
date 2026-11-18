import { Download, Play, Trash2 } from "lucide-react";

import { ConfirmDialog } from "@/components/confirm-dialog";
import { Button } from "@/components/ui/button";
import { ModalShell } from "@/components/ui/modal-shell";
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
    <ConfirmDialog
      open={Boolean(service)}
      onOpenChange={onOpenChange}
      title="Delete service"
      description={`Delete service '${service?.name ?? ""}' from Windows and the config?`}
      icon={<Trash2 className="h-5 w-5 text-destructive" />}
      destructive
      confirmLabel="Delete"
      onConfirm={onConfirm}
    >
      <div className="rounded-md border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">
        This removes the Windows service and its entry in the config. This cannot be undone.
      </div>
    </ConfirmDialog>
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
    <ModalShell
      open={Boolean(service)}
      onOpenChange={onOpenChange}
      title="Install service"
      description={`Install service '${service?.name ?? ""}' now?`}
      icon={<Download className="h-5 w-5 text-primary" />}
      maxWidthClass="sm:max-w-md"
      footerClassName="flex-wrap"
      footer={
        <>
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
        </>
      }
    >
      <div className="flex gap-2 rounded-md border bg-muted/30 p-3 text-sm text-muted-foreground">
        <Play className="mt-0.5 h-4 w-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
        <span>Choose whether to start the service immediately after installation.</span>
      </div>
    </ModalShell>
  );
}
