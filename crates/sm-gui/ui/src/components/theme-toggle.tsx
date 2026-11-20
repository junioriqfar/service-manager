import { Monitor, Moon, Sun, type LucideIcon } from "lucide-react";

import { Hint } from "@/components/hint";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheck,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useTheme, type Theme } from "@/components/theme-provider";

const themeOptions: Array<{ value: Theme; label: string; icon: LucideIcon }> = [
  { value: "light", label: "Light", icon: Sun },
  { value: "dark", label: "Dark", icon: Moon },
  { value: "system", label: "System", icon: Monitor },
];

export function ThemeToggle() {
  const { theme, resolvedTheme, setTheme } = useTheme();
  const CurrentIcon = resolvedTheme === "dark" ? Moon : Sun;

  return (
    <DropdownMenu>
      <Hint label="Change theme" side="left">
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 text-muted-foreground"
            aria-label="Change theme"
          >
            <CurrentIcon className="h-4 w-4" />
          </Button>
        </DropdownMenuTrigger>
      </Hint>
      <DropdownMenuContent align="end" className="w-40">
        <DropdownMenuLabel>Theme</DropdownMenuLabel>
        <DropdownMenuSeparator />
        {themeOptions.map((option) => {
          const Icon = option.icon;
          return (
            <DropdownMenuItem
              key={option.value}
              onSelect={() => setTheme(option.value)}
              className="gap-2"
            >
              <Icon className="h-4 w-4" />
              <span>{option.label}</span>
              <DropdownMenuCheck checked={theme === option.value} />
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
