export { invoke } from "@tauri-apps/api/core";
export { listen, type UnlistenFn } from "@tauri-apps/api/event";

export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
