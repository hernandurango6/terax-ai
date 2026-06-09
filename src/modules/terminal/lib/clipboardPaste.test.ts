import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@/lib/platform", () => ({
  IS_WINDOWS: true,
}));

import { invoke } from "@tauri-apps/api/core";
import {
  formatClipboardImagePath,
  readTerminalPasteText,
} from "./clipboardPaste";

describe("formatClipboardImagePath", () => {
  it("always wraps the path in double quotes", () => {
    expect(formatClipboardImagePath("C:\\Users\\me\\clip.png")).toBe(
      '"C:\\Users\\me\\clip.png" ',
    );
  });

  it("normalizes forward slashes on Windows", () => {
    expect(
      formatClipboardImagePath("C:/Users/me/AppData/Local/terax/clip.png"),
    ).toBe('"C:\\Users\\me\\AppData\\Local\\terax\\clip.png" ');
  });

  it("escapes embedded double quotes", () => {
    expect(formatClipboardImagePath('C:\\tmp\\a"b.png')).toBe(
      '"C:\\tmp\\a""b.png" ',
    );
  });
});

describe("readTerminalPasteText", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.stubGlobal("navigator", {
      clipboard: { readText: vi.fn().mockResolvedValue("plain text") },
    });
  });

  it("pastes quoted image path when clipboard holds a bitmap", async () => {
    vi.mocked(invoke).mockResolvedValue(
      "C:\\Users\\me\\AppData\\Local\\terax\\clipboard\\clipboard_1_0.png",
    );
    await expect(readTerminalPasteText()).resolves.toBe(
      '"C:\\Users\\me\\AppData\\Local\\terax\\clipboard\\clipboard_1_0.png" ',
    );
    expect(invoke).toHaveBeenCalledWith("clipboard_image_to_file");
  });

  it("falls back to text when no image is on the clipboard", async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    await expect(readTerminalPasteText()).resolves.toBe("plain text");
  });
});