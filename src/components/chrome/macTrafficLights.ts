/**
 * macOS Overlay titlebar: vertically center traffic lights with the logo tile.
 *
 * Must stay in sync with:
 * - `src-tauri/tauri.conf.json` / `tauri.macos.conf.json` → windows[0].trafficLightPosition
 * - `AppChrome` header `py-[5px]` (= HEADER_PAD_Y)
 * - brand logo tile `h-[34px]` (= LOGO_TILE_PX)
 */
export const MAC_HEADER_PAD_Y = 5;
export const MAC_LOGO_TILE_PX = 34;
export const MAC_TRAFFIC_LIGHT_H = 12;

export const MAC_TRAFFIC_LIGHT_POSITION = {
  x: 14,
  y: MAC_HEADER_PAD_Y + (MAC_LOGO_TILE_PX - MAC_TRAFFIC_LIGHT_H) / 2,
} as const;

/** Left padding so brand/content clears the traffic-light cluster. */
export const MAC_TRAFFIC_LIGHT_INSET_CLASS = "pl-[76px]";
