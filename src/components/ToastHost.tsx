import { Toaster } from "react-hot-toast";
import {
  MAC_HEADER_PAD_Y,
  MAC_LOGO_TILE_PX,
} from "@/components/chrome/macTrafficLights";

/** Below AppChrome (pad + logo tile + gap), under titlebar z-index. */
const TOAST_TOP_PX = MAC_HEADER_PAD_Y * 2 + MAC_LOGO_TILE_PX + 10;

/** Non-blocking toasts — below custom titlebar, above main content. */
export function ToastHost() {
  return (
    <Toaster
      position="top-center"
      gutter={10}
      containerStyle={{ top: TOAST_TOP_PX, zIndex: 90 }}
      toastOptions={{
        duration: 4500,
        className: "ams-hot-toast",
        style: {
          background: "transparent",
          boxShadow: "none",
          padding: 0,
          maxWidth: "100%",
        },
      }}
    />
  );
}
