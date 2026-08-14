/** Lightweight OS detection for chrome mode (no Tauri dependency). */

export function isWindowsHost(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Win/i.test(navigator.platform) || /Windows/i.test(navigator.userAgent);
}

export function isMacOsHost(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);
}

export function isLinuxHost(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Linux/i.test(navigator.platform) && !/Android/i.test(navigator.userAgent);
}
