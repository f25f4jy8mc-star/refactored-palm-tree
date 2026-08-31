// A real proxy image when one exists, the type glyph otherwise. `thumb_ref`
// is an absolute path on disk — convertFileSrc turns it into a URL the
// webview's asset protocol is actually allowed to load (tauri.conf.json's
// assetProtocol scope + the tauri crate's protocol-asset feature both have
// to be on for this, or the URL 404s silently).
//
// `proxy_state` is the source of truth for whether a thumbnail should be
// there, not just whether `thumb_ref` happens to be non-null — `pending`
// and `failed` both draw the glyph, exactly as the model doc specifies.

import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { IconGlyph } from "./IconGlyph";

type Thumbable = {
  thumb_ref: string | null;
  proxy_state: string;
  icon_kind: string;
};

export function Thumbnail({ item, className }: { item: Thumbable; className?: string }) {
  const [failed, setFailed] = useState(false);
  const usable = item.proxy_state === "ready" && !!item.thumb_ref && !failed;

  // A row can be reused across different files as the list re-renders
  // (React reuses DOM nodes by position in some layouts) — reset the
  // failure flag when the underlying ref actually changes.
  useEffect(() => setFailed(false), [item.thumb_ref]);

  if (!usable) return <IconGlyph kind={item.icon_kind} />;

  return (
    <img
      className={className}
      src={convertFileSrc(item.thumb_ref as string)}
      alt=""
      draggable={false}
      onError={() => setFailed(true)}
    />
  );
}
