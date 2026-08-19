import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { resolveChromeMode, type ChromeMode } from "./chromeMode";
import { isCustomTitlebarEnabled } from "./titlebarFlag";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function useWindowChrome() {
  const [chromeMode, setChromeMode] = useState<ChromeMode>(() =>
    resolveChromeMode(),
  );
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    const mode = resolveChromeMode();
    setChromeMode(mode);

    if (!isTauriRuntime()) return;

    const appWindow = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void (async () => {
      try {
        if (!isCustomTitlebarEnabled()) {
          await appWindow.setDecorations(true);
        } else if (mode === "custom-controls") {
          // Win/Linux: frameless custom chrome (conf starts undecorated).
          await appWindow.setDecorations(false);
        }
        // macos-overlay: leave decorations + Overlay from window create as-is.

        const max = await appWindow.isMaximized();
        if (!cancelled) setIsMaximized(max);

        unlisten = await appWindow.onResized(async () => {
          try {
            const next = await appWindow.isMaximized();
            if (!cancelled) setIsMaximized(next);
          } catch {
            /* ignore */
          }
        });
      } catch {
        /* browser / missing permissions */
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const minimize = () => {
    if (!isTauriRuntime()) return;
    void getCurrentWindow().minimize();
  };

  const toggleMaximize = () => {
    if (!isTauriRuntime()) return;
    void getCurrentWindow().toggleMaximize();
  };

  const close = () => {
    if (!isTauriRuntime()) return;
    void getCurrentWindow().close();
  };

  return {
    chromeMode,
    isMaximized,
    showCustomControls: chromeMode === "custom-controls",
    macOverlayInset: chromeMode === "macos-overlay",
    minimize,
    toggleMaximize,
    close,
  };
}
