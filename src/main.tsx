import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/theme.css";
import { applyWindowEffects, initTheme } from "./services/theme";

// 渲染前先定主题，避免浅色用户看到深色闪屏
initTheme();
void applyWindowEffects();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
