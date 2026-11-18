import type { ComponentProps, ReactNode } from "react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

type DialogContentProps = ComponentProps<typeof DialogContent>;

export interface ModalShellProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  icon?: ReactNode;
  footer?: ReactNode;
  children?: ReactNode;
  maxWidthClass?: string;
  bodyClassName?: string;
  footerClassName?: string;
  headerClassName?: string;
  hideClose?: boolean;
  contentProps?: Omit<DialogContentProps, "className" | "children" | "hideClose">;
}

/**
 * Shared modal frame: a fixed header and footer with a single scrollable body.
 * Every dialog in the app should use this shell (or ConfirmDialog) so the
 * window cannot grow past the viewport and the action buttons stay reachable.
 */
export function ModalShell({
  open,
  onOpenChange,
  title,
  description,
  icon,
  footer,
  children,
  maxWidthClass = "sm:max-w-lg",
  bodyClassName,
  footerClassName,
  headerClassName,
  hideClose = false,
  contentProps,
}: ModalShellProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        hideClose={hideClose}
        className={cn(
          "flex max-h-[90vh] w-full flex-col gap-0 overflow-hidden p-0",
          maxWidthClass,
        )}
        {...contentProps}
      >
        <DialogHeader
          className={cn(
            "shrink-0 space-y-1 border-b px-6 py-4 pr-14 text-left",
            headerClassName,
          )}
        >
          <DialogTitle className="flex items-center gap-2 text-lg leading-tight tracking-tight">
            {icon}
            {title}
          </DialogTitle>
          {description ? (
            <DialogDescription className="text-sm text-muted-foreground">
              {description}
            </DialogDescription>
          ) : null}
        </DialogHeader>

        <div className={cn("min-h-0 flex-1 overflow-y-auto px-6 py-5", bodyClassName)}>
          {children}
        </div>

        {footer ? (
          <DialogFooter
            className={cn(
              "shrink-0 gap-2 border-t bg-muted/20 px-6 py-4 sm:space-x-0",
              footerClassName,
            )}
          >
            {footer}
          </DialogFooter>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
