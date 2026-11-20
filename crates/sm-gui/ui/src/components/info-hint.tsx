import { Info } from "lucide-react";

import { Hint } from "@/components/hint";
import { cn } from "@/lib/utils";

/**
 * Small info icon for form help. Use it instead of a native `title` on
 * labels, inputs, and selects.
 */
export function InfoHint({
  label,
  className,
  side = "top",
}: {
  label: string;
  className?: string;
  side?: "top" | "right" | "bottom" | "left";
}) {
  return (
    <Hint label={label} side={side}>
      <span
        role="img"
        tabIndex={0}
        aria-label={label}
        className={cn(
          "inline-flex shrink-0 cursor-help items-center text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
          className,
        )}
      >
        <Info className="h-3.5 w-3.5" aria-hidden="true" />
      </span>
    </Hint>
  );
}
