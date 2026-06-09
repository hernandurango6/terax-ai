import { invoke } from "@tauri-apps/api/core";
import { IS_WINDOWS } from "@/lib/platform";

/** Tabby-style: always double-quoted so Claude/PowerShell resolve each paste. */
export function formatClipboardImagePath(path: string): string {
  const native = IS_WINDOWS ? path.replace(/\//g, "\\") : path;
  return `"${native.replace(/"/g, '""')}" `;
}

export async function readTerminalPasteText(): Promise<string | null> {
  if (IS_WINDOWS) {
    const path = await invoke<string | null>("clipboard_image_to_file").catch(
      () => null,
    );
    if (path) return formatClipboardImagePath(path);
  }
  const text = await navigator.clipboard.readText().catch(() => "");
  return text || null;
}