// The tag popup — Build 17's tagger, carried across onto the new model.
//
// Left to right: what you are tagging, the three tiers with their facets,
// unclassified, then the collectors you made. The layout is Build 17's and so
// is the rule that makes it worth having: a batch is narrowed *inside* the
// popup, without closing it — click one subject to tag only that, ⌘-click to
// add or take one away, "all" to widen back.
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

import { useCallback, useEffect, useMemo, useState } from "react";

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
import type { Facet, Row, SelectionTags, Tag, TagSuggestion } from "../../lib/types";
import { Thumbnail } from "../library/Thumbnail";

type State = "all" | "some" | undefined;

const chip = (s: State) => "chip" + (s === "all" ? " on" : s === "some" ? " partial" : "");

export function TagPopup({ ids, onClose }: { ids: string[]; onClose: () => void }) {
  const [subjects, setSubjects] = useState<Row[]>([]);
  // Which of the selection this popup is tagging. Starts as all of it.
  const [active, setActive] = useState<string[]>(ids);
  const [facets, setFacets] = useState<Facet[]>([]);
  const [tags, setTags] = useState<Tag[]>([]);
  const [counts, setCounts] = useState<SelectionTags | null>(null);
  const [suggestions, setSuggestions] = useState<TagSuggestion[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const targets = useMemo(() => (active.length > 0 ? active : ids), [active, ids]);

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
                  className="btn quiet tp-all"
                  disabled={active.length === ids.length}
                  onClick={() => setActive(ids)}
                >
                  all
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
            <ul className="tp-list">
              {subjects.map((s) => (
                <li
                  key={s.id}
                  className={targets.includes(s.id) ? "on" : ""}
                  onClick={(e) =>
                    // The same rules as every list: ⌘ toggles, a plain click
                    // isolates. Emptying it falls back to all of them.
                    setActive((prev) => {
                      if (e.metaKey || e.ctrlKey) {
                        const next = prev.includes(s.id)
                          ? prev.filter((x) => x !== s.id)
                          : [...prev, s.id];
                        return next.length ? next : ids;
                      }
                      return [s.id];
                    })
                  }
                >
                  <span className="icon">
                    <Thumbnail item={s} />
                  </span>
                  <span className="tp-list-name">{s.display_name}</span>
                </li>
              ))}
            </ul>
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
