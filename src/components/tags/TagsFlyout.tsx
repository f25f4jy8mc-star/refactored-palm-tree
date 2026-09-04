// Managing the tags themselves — rename, refile, merge, promote, delete.
//
// Deliberately separate from applying them. Applying a tag to items happens
// in the Inspector, against a selection; this panel never touches an item.
// Build 17 ran the two jobs together and it is why "remove this tag" was
// ambiguous between "take it off this photo" and "delete the word".
//
// Three things the panel says out loud rather than leaving to be discovered:
//
//   * Deleting a tag takes it off every item that carried it, and keeps the
//     items.
//   * Promotion to a Collector is one-way. Tags describe; Collectors
//     aggregate, and that distinction is the only thing keeping "things like
//     this" apart from "things I have gathered".
//   * A dismissed near-duplicate never comes back. "singer" and "singers"
//     may well be two different claims.

import { useCallback, useEffect, useMemo, useState } from "react";

import {
  deleteTag,
  dismissSuggestion,
  duplicateTags,
  listFacets,
  listTags,
  mergeTags,
  promoteTag,
  renameTag,
  setTagFacet,
} from "../../lib/api";
import { useArchivaChanged } from "../../lib/events";
import type { DuplicatePair, Facet, Tag } from "../../lib/types";

export function TagsFlyout({ onClose }: { onClose: () => void }) {
  const [tags, setTags] = useState<Tag[]>([]);
  const [facets, setFacets] = useState<Facet[]>([]);
  const [dupes, setDupes] = useState<DuplicatePair[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [mergeFrom, setMergeFrom] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [t, f, d] = await Promise.all([listTags(), listFacets(), duplicateTags()]);
      setTags(t);
      setFacets(f);
      setDupes(d);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  useArchivaChanged(load);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // Escape backs out of the innermost thing first.
      if (editing) setEditing(null);
      else if (mergeFrom) setMergeFrom(null);
      else onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, editing, mergeFrom]);

  async function run(label: string, fn: () => Promise<unknown>) {
    setBusy(label);
    setError(null);
    try {
      await fn();
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  /** Grouped by facet, in the backend's tier order — the vocabulary's own
   * shape, not one invented here. */
  const grouped = useMemo(
    () =>
      facets
        .map((f) => ({ facet: f, tags: tags.filter((t) => t.facet === f.id) }))
        .filter((g) => g.tags.length > 0 || g.facet.tier > 0),
    [facets, tags],
  );

  const byId = (id: string) => tags.find((t) => t.id === id);

  return (
    <div className="flyout wide" onClick={(e) => e.stopPropagation()}>
      <div className="flyout-head">
        <strong>Tags</strong>
        <span className="hint">the vocabulary, not what carries it</span>
      </div>

      {dupes.length > 0 && (
        <div className="dupes">
          <div className="dupes-head">
            Possible duplicates <span className="count">{dupes.length}</span>
          </div>
          {dupes.map((d) => (
            <div className="dupe" key={d.key}>
              <span className="dupe-pair">
                <b>{d.a.name}</b>
                <span className="dupe-arrow">←</span>
                <span>{d.b.name}</span>
              </span>
              <span className="dupe-why">{d.reason}</span>
              <button
                className="btn"
                disabled={!!busy}
                title={`Fold “${d.b.name}” into “${d.a.name}” on every item carrying it`}
                onClick={() => run("Merging…", () => mergeTags(d.b.id, d.a.id))}
              >
                Merge
              </button>
              <button
                className="btn quiet"
                title="Keep both, and never ask again"
                disabled={!!busy}
                onClick={() => run("Dismissing…", () => dismissSuggestion(d.key, "tag_duplicate"))}
              >
                Keep both
              </button>
            </div>
          ))}
        </div>
      )}

      {tags.length === 0 && !busy && (
        <p className="hint">
          No tags yet. Tags are made where they are used — pick an item, and add one in the Inspector.
        </p>
      )}

      {mergeFrom && (
        <p className="batch-note">
          Pick the tag to fold “{byId(mergeFrom)?.name}” into. Everything carrying it will carry the other
          instead, and this one is deleted. Escape to cancel.
        </p>
      )}

      <div className="tag-groups">
        {grouped.map(({ facet, tags: inFacet }) => (
          <div className="tag-group" key={facet.id}>
            <div className="tag-group-head" title={facet.hint}>
              <span>{facet.label}</span>
              <span className="tier-n">tier {facet.tier}</span>
            </div>
            {inFacet.length === 0 ? (
              <div className="hint empty-facet">nothing filed here yet</div>
            ) : (
              <ul className="tag-list">
                {inFacet.map((t) => (
                  <li key={t.id} className={mergeFrom === t.id ? "merging" : ""}>
                    {editing === t.id ? (
                      <form
                        className="tag-rename"
                        onSubmit={(e) => {
                          e.preventDefault();
                          const name = draft.trim();
                          setEditing(null);
                          if (name && name !== t.name) run("Renaming…", () => renameTag(t.id, name));
                        }}
                      >
                        <input
                          autoFocus
                          value={draft}
                          onChange={(e) => setDraft(e.target.value)}
                          onBlur={() => setEditing(null)}
                        />
                      </form>
                    ) : (
                      <button
                        className="tag-name"
                        title="Rename"
                        onClick={() => {
                          if (mergeFrom && mergeFrom !== t.id) {
                            const from = mergeFrom;
                            setMergeFrom(null);
                            run("Merging…", () => mergeTags(from, t.id));
                            return;
                          }
                          setDraft(t.name);
                          setEditing(t.id);
                        }}
                      >
                        {t.name}
                      </button>
                    )}
                    <span className="tag-usage" title="items carrying it">
                      {t.usage}
                    </span>
                    <select
                      className="tag-facet"
                      value={t.facet}
                      title="Move to another facet — the tier follows the facet"
                      disabled={!!busy}
                      onChange={(e) => run("Refiling…", () => setTagFacet(t.id, e.target.value))}
                    >
                      {facets.map((f) => (
                        <option key={f.id} value={f.id}>
                          {f.label}
                        </option>
                      ))}
                    </select>
                    <button
                      className="btn quiet"
                      title="Merge into another tag"
                      disabled={!!busy}
                      onClick={() => setMergeFrom(t.id)}
                    >
                      ⤳
                    </button>
                    <button
                      className="btn quiet"
                      title="Turn into a Collector — one-way"
                      disabled={!!busy || t.usage === 0}
                      onClick={() => run("Promoting…", () => promoteTag(t.id, null, false))}
                    >
                      ⇧
                    </button>
                    <button
                      className="btn quiet"
                      title={`Delete — takes it off ${t.usage} item${t.usage === 1 ? "" : "s"}, keeps them`}
                      disabled={!!busy}
                      onClick={() => run("Deleting…", () => deleteTag(t.id))}
                    >
                      ✕
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        ))}
      </div>

      <div className="flyout-status">{error ? <span className="error">{error}</span> : busy}</div>
    </div>
  );
}
