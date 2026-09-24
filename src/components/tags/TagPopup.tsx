// The tag popup — Build 17's tagger, carried across onto the new model.
//
// Left to right: what you are tagging, the three tiers with their facets,
// unclassified, then the collectors you made. The layout is Build 17's and so
// is the rule that makes it worth having: a batch is narrowed *inside* the
// popup, without closing it.
//
// The subject list is a list like every other in the app, run by the same
// `lib/selection` rules rather than a private copy: click isolates, ⌘-click
// toggles, ⇧-click ranges; ↑/↓ move (⇧ extends), Space toggles the row under
// the cursor, ⌘A and "Select all" widen back to everything. It has focus when
// the popup opens, so the arrows work before anything is clicked.
//
// Every chip reads from `selection_tags`, which counts over the edges once:
// **black** is on every item being tagged, **grey** is on some — and clicking
// grey completes it across all of them rather than taking it off the few that
// had it. Two components counting for themselves could disagree about what
// "all" means; one count in the model cannot (rule 1).
//
// Tagging stays the batch operation C2 asks for. Collectors are here too
// because "where does this go" is asked in the same breath as "what is it";
// only the ones made in Archiva are offered, since a folder mirrored from disk
// holds what the disk says it holds (`relate::gather_target`).

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  acceptSuggestion,
  applyTag,
  createItem,
  createTag,
  gather,
  listFacets,
  listTags,
  nodeRecord,
  removeTag,
  rowsOf,
  selectionTags,
  ungather,
} from "../../lib/api";
import { useArchivaChanged } from "../../lib/events";
import * as Sel from "../../lib/selection";
import type { Facet, Row, SelectionTags, Tag, TagSuggestion } from "../../lib/types";
import { Thumbnail } from "../library/Thumbnail";

type State = "all" | "some" | undefined;

const chip = (s: State) => "chip" + (s === "all" ? " on" : s === "some" ? " partial" : "");

export function TagPopup({ ids, onClose }: { ids: string[]; onClose: () => void }) {
  const [subjects, setSubjects] = useState<Row[]>([]);
  // Which of the selection this popup is tagging. Starts as all of it, with
  // the keyboard cursor on the first.
  const [sel, setSel] = useState<Sel.SelectionState>(() => ({
    ids: new Set(ids),
    anchor: ids[0] ?? null,
    cursor: ids[0] ?? null,
  }));
  const listRef = useRef<HTMLUListElement>(null);
  const rowRefs = useRef<Map<string, HTMLElement>>(new Map());
  const [facets, setFacets] = useState<Facet[]>([]);
  const [tags, setTags] = useState<Tag[]>([]);
  const [counts, setCounts] = useState<SelectionTags | null>(null);
  const [suggestions, setSuggestions] = useState<TagSuggestion[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // In the order the list draws them, which is the order they were selected
  // in. Never empty: a selection narrowed to nothing would tag nothing.
  const targets = useMemo(() => {
    const t = ids.filter((id) => sel.ids.has(id));
    return t.length > 0 ? t : ids;
  }, [ids, sel]);
  const allOn = targets.length === ids.length;

  useEffect(() => {
    listRef.current?.focus();
  }, []);

  /** Apply a selection change, refusing one that would leave nothing. */
  const choose = (next: Sel.SelectionState) => {
    setSel(next.ids.size > 0 ? next : { ...next, ids: new Set(ids) });
    if (next.cursor) rowRefs.current.get(next.cursor)?.scrollIntoView({ block: "nearest" });
  };

  const onListKey = (e: React.KeyboardEvent) => {
    const order = subjects.map((s) => s.id);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      choose(Sel.moveCursor(sel, order, e.key === "ArrowDown" ? 1 : -1, e.shiftKey));
      return;
    }
    if (e.key === " " && sel.cursor) {
      e.preventDefault();
      e.stopPropagation();
      choose(Sel.toggleClick(sel, sel.cursor));
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      e.stopPropagation();
      choose({ ...Sel.selectAll(order), cursor: sel.cursor });
    }
  };

  const load = useCallback(async () => {
    try {
      const [rows, f, t, c] = await Promise.all([
        rowsOf(ids),
        listFacets(),
        listTags(),
        selectionTags(targets),
      ]);
      setSubjects(rows);
      setFacets(f);
      setTags(t);
      setCounts(c);
      // Suggestions are about one item — "this looks like 2019" — so they are
      // offered when exactly one is being tagged, as in Build 17.
      setSuggestions(
        targets.length === 1 ? (await nodeRecord(targets[0])).classification.suggestions : [],
      );
    } catch (e) {
      setError(String(e));
    }
  }, [ids, targets]);

  useEffect(() => {
    load();
  }, [load]);
  useArchivaChanged(load);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const state = (m: Record<string, number> | undefined, id: string): State => {
    const n = m?.[id] ?? 0;
    if (!counts || n === 0) return undefined;
    return n >= counts.total ? "all" : "some";
  };

  /** One place that reports failure. The backend's change event refreshes. */
  const write = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const toggleTag = (t: Tag) =>
    write(() =>
      state(counts?.tags, t.id) === "all" ? removeTag(targets, t.id) : applyTag(targets, t.id),
    );

  const toggleCollector = (id: string) =>
    write(() =>
      state(counts?.collectors, id) === "all" ? ungather(targets, id) : gather(targets, id),
    );

  const addTag = (facet: string) => {
    const name = (drafts[facet] ?? "").trim();
    if (!name) return;
    setDrafts((d) => ({ ...d, [facet]: "" }));
    write(async () => applyTag(targets, await createTag(name, facet)));
  };

  const addCollector = () => {
    const name = (drafts.__collector ?? "").trim();
    if (!name) return;
    setDrafts((d) => ({ ...d, __collector: "" }));
    write(async () => gather(targets, (await createItem("folder", name)).id));
  };

  const facetBlock = (f: Facet) => (
    <div key={f.id} className="tp-facet">
      <div className="tp-facet-head" title={f.hint}>
        {f.label}
      </div>
      <div className="tp-chips">
        {tags
          .filter((t) => t.facet === f.id)
          .map((t) => {
            const s = state(counts?.tags, t.id);
            return (
              <button
                key={t.id}
                className={chip(s)}
                disabled={busy}
                title={
                  s === "some"
                    ? `On ${counts?.tags[t.id]} of ${counts?.total} — click to put it on all`
                    : s === "all"
                      ? "On every one — click to take it off"
                      : undefined
                }
                onClick={() => toggleTag(t)}
              >
                {t.name}
              </button>
            );
          })}
        <input
          className="chip-input"
          placeholder="add…"
          value={drafts[f.id] ?? ""}
          disabled={busy}
          onChange={(e) => setDrafts((d) => ({ ...d, [f.id]: e.target.value }))}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") addTag(f.id);
            if (e.key === "Escape") onClose();
          }}
        />
      </div>
    </div>
  );

  const tiers = [1, 2, 3].map((tier) => ({
    tier,
    label: facets.find((f) => f.tier === tier)?.tier_label ?? `Tier ${tier}`,
    facets: facets.filter((f) => f.tier === tier),
  }));
  const loose = facets.filter((f) => f.tier === 0);
  const shown = subjects.filter((s) => targets.includes(s.id));

  return (
    <div className="dialog-backdrop" onClick={onClose}>
      <div
        className="tag-popup"
        role="dialog"
        aria-label="Tag"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="tp-head">
          <strong>
            Tagging {targets.length} of {ids.length} selected
          </strong>
          {suggestions.length > 0 && (
            <div className="tp-suggest">
              <span className="hint">suggested</span>
              {suggestions.map((s) => (
                <button
                  key={s.key}
                  className="chip suggested"
                  title={`${s.facet} · from ${s.evidence}`}
                  disabled={busy}
                  onClick={() => write(() => acceptSuggestion(targets[0], s.facet, s.name))}
                >
                  + {s.name}
                </button>
              ))}
            </div>
          )}
          <button className="btn quiet" title="Close (Esc)" onClick={onClose}>
            ✕
          </button>
        </div>

        {error && <div className="tp-error error">{error}</div>}

        <div className="tp-body">
          {/* 1 — what is being tagged */}
          <div className="tp-col tp-subjects">
            <div className="tp-col-head">
              Selection
              {ids.length > 1 && (
                <button
                  className="btn tp-all"
                  disabled={allOn}
                  title="Tag every item in the selection (⌘A)"
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => choose({ ...Sel.selectAll(ids), cursor: sel.cursor })}
                >
                  Select all
                </button>
              )}
            </div>
            <div className="tp-preview">
              {shown.slice(0, 9).map((s) => (
                <span key={s.id} className="tp-tile" title={s.display_name}>
                  <Thumbnail item={s} />
                </span>
              ))}
            </div>
            <ul
              ref={listRef}
              className="tp-list"
              tabIndex={0}
              role="listbox"
              aria-multiselectable
              aria-label="What is being tagged"
              onKeyDown={onListKey}
            >
              {subjects.map((s) => {
                const on = targets.includes(s.id);
                return (
                  <li
                    key={s.id}
                    ref={(el) => {
                      if (el) rowRefs.current.set(s.id, el);
                      else rowRefs.current.delete(s.id);
                    }}
                    role="option"
                    aria-selected={on}
                    className={(on ? "on" : "") + (sel.cursor === s.id ? " cursor" : "")}
                    onMouseDown={(e) => {
                      // Keep focus on the list, so the arrows carry on from here.
                      e.preventDefault();
                      listRef.current?.focus();
                    }}
                    onClick={(e) => {
                      const order = subjects.map((x) => x.id);
                      if (e.shiftKey) choose(Sel.rangeClick(sel, s.id, order));
                      else if (e.metaKey || e.ctrlKey) choose(Sel.toggleClick(sel, s.id));
                      else choose(Sel.click(s.id));
                    }}
                  >
                    <span className="tp-check" aria-hidden>
                      {on ? "✓" : ""}
                    </span>
                    <span className="icon">
                      <Thumbnail item={s} />
                    </span>
                    <span className="tp-list-name">{s.display_name}</span>
                  </li>
                );
              })}
            </ul>
            <div className="tp-keys hint">↑↓ move · ⇧ extends · Space toggles · ⌘A all</div>
          </div>

          {/* 2, 3, 4 — the tiers */}
          {tiers.map((t) => (
            <div key={t.tier} className="tp-col">
              <div className="tp-col-head">
                <span className="tier-num">{t.tier}</span> {t.label}
              </div>
              {t.facets.map(facetBlock)}
            </div>
          ))}

          {/* 5 — unclassified */}
          {loose.length > 0 && (
            <div className="tp-col">
              <div className="tp-col-head">
                <span className="tier-num">?</span> Unclassified
              </div>
              {loose.map(facetBlock)}
            </div>
          )}

          {/* 6 — collectors you made */}
          <div className="tp-col">
            <div className="tp-col-head">
              <span className="tier-num">◆</span> Collectors
            </div>
            <div className="tp-facet">
              <div className="tp-chips">
                {counts?.targets.map((c) => (
                  <button
                    key={c.id}
                    className={chip(state(counts.collectors, c.id)) + " collector"}
                    disabled={busy}
                    onClick={() => toggleCollector(c.id)}
                  >
                    {c.kind === "board" ? "▢" : "▤"} {c.name}
                  </button>
                ))}
                <input
                  className="chip-input"
                  placeholder="new folder…"
                  value={drafts.__collector ?? ""}
                  disabled={busy}
                  onChange={(e) => setDrafts((d) => ({ ...d, __collector: e.target.value }))}
                  onKeyDown={(e) => {
                    e.stopPropagation();
                    if (e.key === "Enter") addCollector();
                    if (e.key === "Escape") onClose();
                  }}
                />
              </div>
            </div>
          </div>
        </div>

        <div className="tp-foot">
          <span className="hint">
            black = on every one being tagged · grey = on some, click to put it on all
          </span>
          <button className="btn primary" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
