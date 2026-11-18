import { useState } from "react";
import { FileSearch, FolderOpen, Loader2, Save } from "lucide-react";
import { toast } from "sonner";

import { Hint } from "@/components/hint";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { errorMessage, invoke } from "@/lib/tauri";
import { cn } from "@/lib/utils";

export type PathInputMode = "file" | "directory" | "executable" | "save";

export interface PathInputProps {
  id?: string;
  label?: string;
  value: string;
  onChange: (value: string) => void;
  /**
   * Called only after a browse selection (not when the user types), so forms
   * can infer related fields such as display name or working directory.
   */
  onBrowsed?: (value: string) => void;
  mode: PathInputMode;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  inputClassName?: string;
  helpText?: string;
  hint?: string;
  filterName?: string;
  extensions?: string[];
  defaultFilename?: string;
  onError?: (message: string) => void;
  showLabel?: boolean;
}

const MODE_ICON = {
  file: FileSearch,
  directory: FolderOpen,
  executable: FileSearch,
  save: Save,
} as const;

function browseActionLabel(mode: PathInputMode): string {
  switch (mode) {
    case "directory":
      return "Browse for a folder";
    case "executable":
      return "Browse for an executable";
    case "save":
      return "Choose a save location";
    default:
      return "Browse for a file";
  }
}

/**
 * Path field with an icon-only browse button. Browse is available for local
 * Windows paths only; server-side paths must stay plain Inputs.
 */
export function PathInput({
  id,
  label,
  value,
  onChange,
  onBrowsed,
  mode,
  placeholder,
  disabled = false,
  className,
  inputClassName,
  helpText,
  hint,
  filterName,
  extensions,
  defaultFilename,
  onError,
  showLabel = true,
}: PathInputProps) {
  const [browsing, setBrowsing] = useState(false);
  const Icon = MODE_ICON[mode];
  const action = browseActionLabel(mode);
  const tooltip = hint ?? (label ? `${action}: ${label}` : action);
  const accessibleLabel = label ? `${action}: ${label}` : tooltip;

  async function browse() {
    if (disabled || browsing) return;
    setBrowsing(true);
    try {
      const current = value.trim() || null;
      const filterArgs = {
        filterName: filterName ?? null,
        extensions: extensions && extensions.length > 0 ? extensions : null,
      };
      const selected =
        mode === "directory"
          ? await invoke<string | null>("pick_directory", { current })
          : mode === "executable"
            ? await invoke<string | null>("pick_executable", { current })
            : mode === "save"
              ? await invoke<string | null>("pick_save_file", {
                  current,
                  defaultFilename: defaultFilename ?? null,
                  ...filterArgs,
                })
              : await invoke<string | null>("pick_file", {
                  current,
                  ...filterArgs,
                });
      if (!selected) return;
      onChange(selected);
      onBrowsed?.(selected);
    } catch (error) {
      if (onError) onError(errorMessage(error));
      else toast.error(errorMessage(error));
    } finally {
      setBrowsing(false);
    }
  }

  return (
    <div className={cn("space-y-1.5", className)}>
      {showLabel && label ? <Label htmlFor={id}>{label}</Label> : null}
      <div className="flex items-center gap-2">
        <Input
          id={id}
          value={value}
          placeholder={placeholder}
          disabled={disabled}
          onChange={(event) => onChange(event.target.value)}
          className={cn("min-w-0 flex-1", inputClassName)}
        />
        <Hint label={tooltip}>
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="h-9 w-9 shrink-0"
            aria-label={accessibleLabel}
            title={tooltip}
            onClick={() => void browse()}
            disabled={disabled || browsing}
          >
            {browsing ? <Loader2 className="animate-spin" /> : <Icon />}
          </Button>
        </Hint>
      </div>
      {helpText ? <p className="text-xs text-muted-foreground">{helpText}</p> : null}
    </div>
  );
}
