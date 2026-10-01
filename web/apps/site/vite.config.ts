import { defineConfig, type ProxyOptions } from "vite";
import react from "@vitejs/plugin-react";
import { env } from "node:process";
import type { ClientRequest, IncomingMessage } from "node:http";

const target = new URL(env.GFA_API_TARGET || "http://127.0.0.1:8080");
if (target.protocol !== "http:" || !["127.0.0.1", "localhost", "[::1]"].includes(target.hostname)) {
  throw new Error("GFA_API_TARGET must be a local HTTP backend");
}
const localProxy: ProxyOptions = {
  target: target.origin,
  changeOrigin: true,
  ws: true,
  configure(proxy) {
    const origin = (outgoing: ClientRequest, incoming: IncomingMessage) => {
      // Rewrite only same-origin browser requests. Foreign origins still reach
      // the backend's access guard unchanged and are rejected.
      if (incoming.headers.origin === `http://${incoming.headers.host}`) {
        outgoing.setHeader("origin", target.origin);
      }
    };
    proxy.on("proxyReq", origin);
    proxy.on("proxyReqWs", origin);
  },
};

export default defineConfig({
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    strictPort: true,
    proxy: { "/v1": localProxy, "/docs": localProxy, "/healthz": localProxy },
  },
});
