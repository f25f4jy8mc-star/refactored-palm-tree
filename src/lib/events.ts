import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

/**
 * Cross-pane invalidation: Rust emits this once after a scan; every open
 * view refetches itself independently rather than the scanner or the rail's
 * "Add Folder" button needing a reference to whichever panes happen to be
 * open. One writer, one event, per invariant 10 of the model.
 */
export function useArchivaChanged(onChanged: () => void) {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen("archiva:changed", () => onChanged()).then((un) => {
      if (cancelled) un();
      else unlisten = un;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [onChanged]);
}
