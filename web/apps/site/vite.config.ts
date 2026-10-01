import { defineConfig, loadEnv, type ProxyOptions } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ mode }) => {
  const environment = loadEnv(mode, ".", "GFA_");
  const target = new URL(environment.GFA_API_TARGET || "http://127.0.0.1:8080");
  if (
    target.protocol !== "http:" ||
    !["127.0.0.1", "localhost", "[::1]"].includes(target.hostname)
  ) {
    throw new Error("GFA_API_TARGET must be a local HTTP backend");
  }
  const localProxy: ProxyOptions = {
    target: target.origin,
    changeOrigin: true,
    ws: true,
    configure(proxy) {
      const origin = (
        outgoing: { setHeader: (name: string, value: string) => unknown },
        incoming: { headers: { origin?: string; host?: string } },
      ) => {
        // Reject every foreign origin, including the backend's own origin.
        // Forwarding it unchanged would let the backend accept that request.
        if (incoming.headers.origin !== undefined) {
          outgoing.setHeader(
            "origin",
            incoming.headers.origin === `http://${incoming.headers.host}`
              ? target.origin
              : "null",
          );
        }
      };
      proxy.on("proxyReq", origin);
      proxy.on("proxyReqWs", origin);
    },
  };
  return {
    plugins: [react()],
    server: {
      host: "127.0.0.1",
      strictPort: true,
      proxy: { "/v1": localProxy, "/docs": localProxy, "/healthz": localProxy },
    },
  };
});
