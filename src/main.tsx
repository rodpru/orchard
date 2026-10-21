import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./i18n";
import { isMac } from "./lib/platform";

document.body.classList.toggle("platform-mac", isMac);

// StrictMode is safe: terminals attach to backend sessions, so a double
// mount reattaches instead of spawning every agent twice.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
