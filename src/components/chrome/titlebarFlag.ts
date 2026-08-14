/** localStorage key — set to `"0"` to disable custom titlebar (dev rollback). */
export const CUSTOM_TITLEBAR_STORAGE_KEY = "ams-custom-titlebar";

/** Default on. Disable with `localStorage.setItem('ams-custom-titlebar', '0')` then reload. */
export function isCustomTitlebarEnabled(): boolean {
  if (typeof localStorage === "undefined") return true;
  try {
    return localStorage.getItem(CUSTOM_TITLEBAR_STORAGE_KEY) !== "0";
  } catch {
    return true;
  }
}
