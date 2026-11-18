import { useEffect, useState } from "react";
import { Import as ImportIcon, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ModalShell } from "@/components/ui/modal-shell";
import { ScrollArea } from "@/components/ui/scroll-area";
import { typeLabel } from "@/lib/service";
import type { ServiceConfig } from "@/lib/types";

interface ImportDialogProps {
  open: boolean;
  candidates: ServiceConfig[];
  importing: boolean;
  onOpenChange: (open: boolean) => void;
  onImport: (names: string[]) => void;
}

export function ImportDialog({
  open,
  candidates,
  importing,
  onOpenChange,
  onImport,
}: ImportDialogProps) {
  const [selected, setSelected] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (open) {
      setSelected(new Set(candidates.map((candidate) => candidate.name)));
    }
  }, [open, candidates]);

  function toggle(name: string, checked: boolean) {
    setSelected((previous) => {
      const next = new Set(previous);
      if (checked) next.add(name);
      else next.delete(name);
      return next;
    });
  }

  const allSelected = candidates.length > 0 && selected.size === candidates.length;

  function toggleAll() {
    setSelected(
      allSelected ? new Set() : new Set(candidates.map((candidate) => candidate.name)),
    );
  }

  return (
    <ModalShell
      open={open}
      onOpenChange={onOpenChange}
      title="Import Services"
      description="Detected NSSM/Apache/PostgreSQL services that are not in the config."
      icon={<ImportIcon className="h-5 w-5 text-primary" />}
      maxWidthClass="sm:max-w-xl"
      footerClassName="items-center gap-3 sm:justify-between"
      footer={
        <>
          <span className="text-xs text-muted-foreground">
            {candidates.length} candidate(s) · {selected.size} selected
          </span>
          <div className="flex w-full justify-end gap-2 sm:w-auto">
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
              disabled={importing}
            >
              Cancel
            </Button>
            <Button
              type="button"
              onClick={() => onImport([...selected])}
              disabled={importing}
            >
              {importing ? <Loader2 className="animate-spin" /> : <ImportIcon />}
              Import
            </Button>
          </div>
        </>
      }
    >
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <p className="text-xs text-muted-foreground">Select the services to add to the config.</p>
          <Button type="button" variant="ghost" size="sm" onClick={toggleAll} disabled={importing}>
            {allSelected ? "Clear all" : "Select all"}
          </Button>
        </div>

        <ScrollArea className="h-[320px] rounded-md border">
          <div className="space-y-1 p-2">
            {candidates.map((service, index) => {
              const checkboxId = `import-candidate-${index}`;
              return (
                <div
                  key={service.name}
                  className="flex items-start gap-3 rounded-md px-2 py-2 hover:bg-muted/60"
                >
                  <Checkbox
                    id={checkboxId}
                    checked={selected.has(service.name)}
                    onCheckedChange={(checked) => toggle(service.name, checked === true)}
                    disabled={importing}
                    className="mt-0.5"
                  />
                  <label
                    htmlFor={checkboxId}
                    className="min-w-0 flex-1 cursor-pointer text-sm leading-snug"
                  >
                    <span className="block truncate font-medium">
                      {service.displayName || service.name}
                    </span>
                    <span className="block truncate text-xs text-muted-foreground">
                      [{service.name}] {typeLabel(service.serviceType)}
                    </span>
                  </label>
                </div>
              );
            })}
          </div>
        </ScrollArea>
      </div>
    </ModalShell>
  );
}
