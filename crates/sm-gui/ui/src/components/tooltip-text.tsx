import type { ReactNode } from "react";

import { Hint } from "@/components/hint";
import { cn } from "@/lib/utils";

/**
 * Truncated text with its full value exposed through the shadCN tooltip.
 * Native `title` attributes are not used anywhere in the UI.
 */
export function TooltipText({
  text,
  children,
  className,
  side,
}: {
  text: string;
  children?: ReactNode;
  className?: string;
  side?: "top" | "right" | "bottom" | "left";
}) {
  const content = children ?? text;
  if (!text) {
    return <span className={className}>{content}</span>;
  }
  return (
    <Hint label={text} side={side}>
      <span className={cn("block min-w-0 truncate", className)}>{content}</span>
    </Hint>
  );
}
