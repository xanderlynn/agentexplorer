import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import "./index.css";
import { applyAppearance, loadAppearance } from "./theme";

applyAppearance(loadAppearance());

const queryClient = new QueryClient({
  defaultOptions: {
    // Data only changes when the Rust side ingests, which emits an event the
    // app listens for, so there is no need to refetch on focus.
    // Commands are local IPC, so the OS's online/offline state is irrelevant.
    queries: { refetchOnWindowFocus: false, staleTime: 30_000, retry: 1, networkMode: "always" },
    mutations: { networkMode: "always" },
  },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
