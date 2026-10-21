import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./i18n";

// StrictMode is safe: terminals attach to backend sessions, so a double
// mount reattaches instead of spawning every agent twice.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
