import { useSyncExternalStore } from "react";

function subscribe(callback: () => void) {
  window.addEventListener("hashchange", callback);
  return () => window.removeEventListener("hashchange", callback);
}

export function useRoute() {
  return useSyncExternalStore(
    subscribe,
    () => window.location.hash.slice(1) || "/",
    () => "/",
  );
}
