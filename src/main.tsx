import ReactDOM from "react-dom/client";
import App from "./App";
import { AppearanceProvider, applyAppearance, loadAppearance } from "./lib/appearance";

// Apply the saved theme before first paint so there's no flash of the default.
applyAppearance(loadAppearance());

// No StrictMode: its dev-only double-invoke of effects would create and dispose
// each xterm Terminal twice on mount, racing the FitAddon.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <AppearanceProvider>
    <App />
  </AppearanceProvider>,
);
