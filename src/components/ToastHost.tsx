import { Toaster } from "react-hot-toast";

/** Non-blocking toasts — bottom-right, above main content (z-index above modal dialogs). */
export function ToastHost() {
  return (
    <Toaster
      position="bottom-right"
      gutter={10}
      containerStyle={{ zIndex: 90 }}
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
