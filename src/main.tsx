import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: {
    // Data only changes when the Rust side ingests, which emits an event the
    // app listens for, so there is no need to refetch on focus.
    queries: { refetchOnWindowFocus: false, staleTime: 30_000, retry: 1 },
  },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
