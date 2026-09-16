// What an item looks like, at whatever size it is given.
//
// One component, two hosts: Quick Look (Space) and the Inspector's head. The
// old build had a renderer in each place and they disagreed about which files
// could be shown, which is the shape this rebuild exists to remove.
//
// What to draw is `previewKind`, which reads the item's resolved capabilities
// — the same ordered rule a double-click follows. Nothing here compares a
// file extension: a HEIC is an image, an MOV plays and a PDF paginates
// because the conformance closure says so (G17), and the *instance* half of
// each grant has already asked whether the file is actually reachable.
//
// `compact` is the Inspector's collapsed head: one still, at thumbnail size.
// A 56px video transport would be a joke, and a PDF in 56px is a grey square.

import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { noteBody } from "../../lib/api";
import { previewKind, previewSource, type PreviewKind } from "../../lib/capabilities";
import type { NoteBody, Row } from "../../lib/types";
import { IconGlyph } from "../library/IconGlyph";

export type Stageable = {
  node: Row;
  locator: string | null;
  previewRef: string | null;
  thumbRef: string | null;
  /** p_record has one; p_detail does not. A transcode to something the
   * webview can decode, for when the original is a codec it cannot. */
  playableRef?: string | null;
};

/** What a kind that cannot be drawn should say, in full. Each names the view
 * that would draw it, so "not yet" never reads as "broken". */
const UNBUILT: Partial<Record<PreviewKind, string>> = {
  model: "3D viewing isn’t built yet — that needs the Orbit view.",
  none: "No preview for this one.",
};

function Placeholder({ node, text }: { node: Row; text: string }) {
  return (
    <div className="preview-placeholder">
      <div className="preview-glyph">
        <IconGlyph kind={node.icon_kind} />
      </div>
      <div>{text}</div>
    </div>
  );
}

/** A note's text, read when a note is what is being shown and not before. */
function TextStage({ node }: { node: Row }) {
  const [body, setBody] = useState<NoteBody | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setBody(null);
    setError(null);
    noteBody(node.id)
      .then((b) => !cancelled && setBody(b))
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [node.id]);

  if (error) return <Placeholder node={node} text={error} />;
  if (!body) return <div className="preview-loading">Reading…</div>;
  return (
    <div className="preview-text">
      <pre>{body.text}</pre>
      {body.truncated && (
        <div className="preview-cut">
          Showing the first part of a long file — the rest is on disk.
        </div>
      )}
    </div>
  );
}

export function PreviewStage({
  item,
  compact = false,
}: {
  item: Stageable;
  compact?: boolean;
}) {
  const [failed, setFailed] = useState(false);
  const { node } = item;
  const kind = previewKind(node);
  const still = previewSource({
    capabilities: node.capabilities,
    locator: item.locator,
    previewRef: item.previewRef,
    thumbRef: item.thumbRef,
  });
  // Playing needs something the webview can decode. `play` is granted when
  // there is a transcode *or* the original is in a native codec, so one of
  // these two is the reason it was granted at all.
  const playable = item.playableRef ?? item.locator;

  // A new item is a new file; the last one's failure says nothing about it.
  useEffect(() => setFailed(false), [node.id, still]);

  // The collapsed head shows a still and nothing else — and for a kind with
  // no still of its own, its glyph. A transport at 56px is not a preview.
  if (compact) {
    if (kind === "image" && still && !failed) {
      return (
        <img
          className="preview-still"
          src={convertFileSrc(still)}
          alt=""
          draggable={false}
          onError={() => setFailed(true)}
        />
      );
    }
    return <IconGlyph kind={node.icon_kind} />;
  }

  if (failed) {
    return <Placeholder node={node} text="That file couldn’t be loaded." />;
  }

  switch (kind) {
    case "image":
      return still ? (
        <img
          className="preview-still"
          src={convertFileSrc(still)}
          alt=""
          draggable={false}
          onError={() => setFailed(true)}
        />
      ) : (
        <Placeholder node={node} text="No preview for this one." />
      );

    case "video":
      return playable ? (
        <video
          className="preview-player"
          src={convertFileSrc(playable)}
          controls
          // Not autoplay: a preview that starts making noise the moment you
          // arrow onto it is a preview you stop using.
          preload="metadata"
          poster={still ? convertFileSrc(still) : undefined}
          onError={() => setFailed(true)}
        />
      ) : (
        <Placeholder node={node} text="Nothing to play — the file isn’t reachable." />
      );

    case "audio":
      return (
        <div className="preview-audio">
          <div className="preview-glyph">
            <IconGlyph kind={node.icon_kind} />
          </div>
          <div className="preview-audio-name">{node.display_name}</div>
          {playable ? (
            <audio
              className="preview-player"
              src={convertFileSrc(playable)}
              controls
              preload="metadata"
              onError={() => setFailed(true)}
            />
          ) : (
            <div>Nothing to play — the file isn’t reachable.</div>
          )}
        </div>
      );

    case "pdf":
      // `paginate` is granted only when the page count is at least one, so a
      // PDF that reaches here has pages. The webview's own viewer draws them;
      // a page-at-a-time reader with its own controls is V6's business.
      return item.locator ? (
        <object
          className="preview-pdf"
          data={convertFileSrc(item.locator)}
          type="application/pdf"
        >
          <Placeholder node={node} text="This PDF can’t be displayed here." />
        </object>
      ) : (
        <Placeholder node={node} text="The file isn’t reachable." />
      );

    case "text":
      return <TextStage node={node} />;

    default:
      return <Placeholder node={node} text={UNBUILT[kind] ?? "No preview for this one."} />;
  }
}
