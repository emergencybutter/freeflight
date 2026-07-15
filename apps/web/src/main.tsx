import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { AboutPage } from "./AboutPage";
import { SettingsPage } from "./SettingsPage";
import "./index.css";

const path = window.location.pathname;
const page =
  path === "/about" ? <AboutPage /> : path === "/settings" ? <SettingsPage /> : <App />;

createRoot(document.getElementById("root")!).render(<StrictMode>{page}</StrictMode>);
