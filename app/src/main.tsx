import React from "react";
import ReactDOM from "react-dom/client";
import { api } from "./api";
import App from "./App";
import { UiProvider } from "./components/ui";
import { asPref, I18nProvider, LangPref } from "./i18n";
import "./styles.css";

function render(lang: LangPref) {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <I18nProvider initial={lang}>
        <UiProvider>
          <App />
        </UiProvider>
      </I18nProvider>
    </React.StrictMode>,
  );
}

// Read the saved language first so the window never flashes the other one.
api.language().then(
  (l) => render(asPref(l)),
  () => render("system"),
);
