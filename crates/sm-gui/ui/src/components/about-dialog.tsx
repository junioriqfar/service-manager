import { Info } from "lucide-react";

import { Button } from "@/components/ui/button";
import { ModalShell } from "@/components/ui/modal-shell";
import type { PathsView } from "@/lib/types";

interface AboutDialogProps {
  open: boolean;
  paths: PathsView | null;
  onOpenChange: (open: boolean) => void;
}

export function AboutDialog({ open, paths, onOpenChange }: AboutDialogProps) {
  const rows: Array<[string, string]> = paths
    ? [
        ["Home", paths.home],
        ["Services config", paths.servicesPath],
        ["Projects config", paths.projectsPath],
        ["NSSM path", paths.nssmPath],
        ["Application log", paths.appLog],
      ]
    : [];

  return (
    <ModalShell
      open={open}
      onOpenChange={onOpenChange}
      title="About Service Manager"
      description="Service Manager Rust version 0.1.0"
      icon={<Info className="h-5 w-5 text-primary" />}
      maxWidthClass="sm:max-w-lg"
      footer={
        <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
          Close
        </Button>
      }
    >
      <div className="space-y-4">
        <div className="space-y-2 rounded-md border bg-muted/30 p-4 text-xs">
          {rows.map(([label, value]) => (
            <div key={label} className="grid grid-cols-[132px_1fr] gap-2">
              <span className="text-muted-foreground">{label}</span>
              <span className="break-all font-mono">{value}</span>
            </div>
          ))}
        </div>
        <p className="text-sm text-muted-foreground">
          The application must run as Administrator to change services.
        </p>
      </div>
    </ModalShell>
  );
}
