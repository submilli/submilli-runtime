import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { createStore } from "./store/store";
import "./styles/page.css";

// One store for the page's life: it signs in from the link and follows the session.
const store = createStore();
void store.start();

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App store={store} />
    </StrictMode>,
  );
}
