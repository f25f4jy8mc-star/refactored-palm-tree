// Making something new (⌘N) — Build 17's create palette, carried across.
//
// Type first, then a name: pick with a click or ⌥ and the type's key, write
// the name, press ⏎. A pasted web address needs no type picked — it can only
// be a link, so asking would be a question with one answer.
//
// Where it lands is said before it is made. Inside a collector you made, a new
// thing goes in that collector unless you untick it; anywhere else it goes to
// the library's top level. A note is a real `.md` file in this space's
// `notes/` folder (S9), which is also said, because "where is the file" is a
// fair question about something that claims to be one.
//
// Not offered, deliberately: importing a single file from disk. A file enters
// the library by being in a linked folder, so the scan that keeps it current
// can find it again — an item added from outside every linked folder would be
// one the next Refresh knows nothing about.

import { useEffect, useRef, useState } from "react";

import { createItem } from "../../lib/api";
import type { GatherTarget, NewKind, Row } from "../../lib/types";

const KINDS: { kind: NewKind; key: string; label: string; glyph: string; hint: string }[] = [
  { kind: "note", key: "n", label: "Note", glyph: "✎", hint: "A markdown file in this space's notes folder" },
  { kind: "folder", key: "f", label: "Folder", glyph: "▤", hint: "A collector to gather things in" },
  { kind: "board", key: "b", label: "Board", glyph: "▢", hint: "A collector laid out as a canvas" },
  { kind: "link", key: "l", label: "Link", glyph: "↗", hint: "Something at a web address" },
];

const isUrl = (s: string) => /^https?:\/\//i.test(s.trim());

export function CreatePalette({
  scope,
  onCreated,
  onClose,
}: {
  /** The collector the active pane is inside, when it can take members. */
  scope: GatherTarget | null;
  onCreated: (row: Row) => void;
  onClose: () => void;
}) {
  const [kind, setKind] = useState<NewKind | null>(null);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [inside, setInside] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const nameRef = useRef<HTMLInputElement>(null);

  // A pasted address decides the type by itself.
  const effective: NewKind | null = kind ?? (isUrl(name) ? "link" : null);
  const into = scope && inside ? scope : null;

  useEffect(() => {
    nameRef.current?.focus();
  }, [kind]);

  async function make() {
    if (!effective || busy) return;
    // For a link the address may be in the name field (pasted straight in)
    // or in its own; the title is whatever else was written.
    const address = effective === "link" ? (isUrl(name) ? name.trim() : url.trim()) : null;
    if (effective === "link" && !address) {
      setError("A link needs a web address.");
      return;
    }
    const title = effective === "link" && isUrl(name) ? "" : name;
    setBusy(true);
    try {
      const row = await createItem(effective, title, { url: address, into: into?.id ?? null });
      onCreated(row);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  function onKey(e: React.KeyboardEvent) {
    // The palette owns its keys while open.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
      return;
    }
    if (e.altKey) {
      const k = e.code.replace(/^Key/, "").toLowerCase();
      const hit = KINDS.find((x) => x.key === k);
      if (hit) {
        e.preventDefault();
        setKind(hit.kind);
        setError(null);
        return;
      }
    }
    if (e.key === "Enter") {
      e.preventDefault();
      make();
    }
  }

  const spec = KINDS.find((k) => k.kind === effective);

  return (
    <div className="dialog-backdrop" onClick={onClose}>
      <div
        className="create-palette"
        role="dialog"
        aria-label="Make something new"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={onKey}
      >
        <div className="cp-input">
          {spec && <span className="cp-chip">{spec.label}</span>}
          <input
            ref={nameRef}
            value={name}
            disabled={busy}
            placeholder={
              effective
                ? effective === "link"
                  ? "Title, or paste the address here…"
                  : `Name the ${spec?.label.toLowerCase()}, then ⏎`
                : "Pick a type below (⌥ and its key), or paste a web address…"
            }
            onChange={(e) => {
              setName(e.target.value);
              setError(null);
            }}
          />
        </div>
        {effective === "link" && !isUrl(name) && (
          <div className="cp-input">
            <span className="cp-chip quiet">URL</span>
            <input
              value={url}
              disabled={busy}
              placeholder="https://…"
              onChange={(e) => {
                setUrl(e.target.value);
                setError(null);
              }}
            />
          </div>
        )}

        <div className="cp-kinds">
          {KINDS.map((k) => (
            <button
              key={k.kind}
              className={"cp-kind" + (effective === k.kind ? " on" : "")}
              title={k.hint}
              data-kind={k.kind}
              disabled={busy}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                setKind(k.kind);
                setError(null);
              }}
            >
              <span className="cp-glyph">{k.glyph}</span>
              {k.label}
              <kbd>⌥{k.key.toUpperCase()}</kbd>
            </button>
          ))}
        </div>

        <div className="cp-where">
          {scope ? (
            <label>
              <input
                type="checkbox"
                checked={inside}
                onChange={(e) => setInside(e.target.checked)}
              />
              Inside <b>{scope.name}</b>
            </label>
          ) : (
            <span className="hint">Goes to the top of the library.</span>
          )}
          {effective === "note" && (
            <span className="hint">Saved as a .md file in this space's notes folder.</span>
          )}
        </div>

        {error && <div className="cp-error error">{error}</div>}

        <div className="cp-foot">
          <button className="btn quiet" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button className="btn primary" onClick={make} disabled={!effective || busy}>
            {effective ? `Make ${spec?.label.toLowerCase()}` : "Make"}
          </button>
        </div>
      </div>
    </div>
  );
}
