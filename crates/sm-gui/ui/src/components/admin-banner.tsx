import { ShieldAlert, ShieldCheck } from "lucide-react";

import { Button } from "@/components/ui/button";

interface AdminBannerProps {
  onElevate: () => void;
}

export function AdminBanner({ onElevate }: AdminBannerProps) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-2 border-b border-amber-300/70 bg-amber-50 px-4 py-2 text-sm text-amber-900 dark:border-amber-500/30 dark:bg-amber-950/40 dark:text-amber-200">
      <div className="flex items-center gap-2">
        <ShieldAlert className="h-4 w-4 shrink-0" />
        <span>
          The application is not running as Administrator. Install/Start/Stop/Restart will
          fail.
        </span>
      </div>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={onElevate}
        className="border-amber-400/80 bg-amber-100/60 text-amber-900 hover:bg-amber-100 hover:text-amber-950 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-100 dark:hover:bg-amber-500/20"
      >
        <ShieldCheck />
        Run as Administrator
      </Button>
    </div>
  );
}
