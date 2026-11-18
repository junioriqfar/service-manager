import type { ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { ModalShell } from "@/components/ui/modal-shell";

export interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  icon?: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
  loading?: boolean;
  onConfirm: () => void;
  maxWidthClass?: string;
}

const destructiveIcon = <AlertTriangle className="h-5 w-5 text-destructive" />;
const defaultIcon = <CheckCircle2 className="h-5 w-5 text-primary" />;

/**
 * Small confirmation modal built on ModalShell. Use destructive for actions
 * that delete or overwrite data; the confirm button then uses the destructive
 * variant and the footer stays visible while the action is loading.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  icon,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  destructive = false,
  loading = false,
  onConfirm,
  maxWidthClass = "sm:max-w-md",
}: ConfirmDialogProps) {
  return (
    <ModalShell
      open={open}
      onOpenChange={(nextOpen) => {
        if (!loading) onOpenChange(nextOpen);
      }}
      title={title}
      description={description}
      icon={icon ?? (destructive ? destructiveIcon : defaultIcon)}
      maxWidthClass={maxWidthClass}
      bodyClassName={children ? "space-y-3" : "py-2"}
      footer={
        <>
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={loading}
          >
            {cancelLabel}
          </Button>
          <Button
            type="button"
            variant={destructive ? "destructive" : "default"}
            onClick={onConfirm}
            disabled={loading}
          >
            {loading ? <Loader2 className="animate-spin" /> : null}
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
    </ModalShell>
  );
}
