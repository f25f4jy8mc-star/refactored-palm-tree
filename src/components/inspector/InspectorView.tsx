// The Inspector: everything `p_record` knows about one item.
//
// It follows the active item rather than owning a selection of its own —
// that was a real bug in the old build, where a viewer pane overwrote the
// inspector's item on every focus change.
//
// One read, one projection. `p_record` wraps p_detail rather than the view
// making five calls and assembling the answer itself, which would be the
// "two components deciding the same fact" shape the rebuild exists to
// remove. Everything below is rendering, never computing: the health score,
// the facet grid and the rule names all arrive decided.
//
// Writing is limited to classification, and deliberately: applying and
// removing tags has a named write path (checklist C2), and rename,
// set-attribute and delete do not exist yet. A field that looks editable but
// silently discards what you type is worse than one that plainly isn't
// offered.

import { createPortal } from "react-dom";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  acceptSuggestion,
  applyTag,
  createTag,
  dismissSuggestion,
  listTags,
  nodeRecord,
  removeTag,
} from "../../lib/api";
import { useActiveItem } from "../../lib/activeItem";
import { useArchivaChanged } from "../../lib/events";
import { CAPABILITY_LABEL, type Capability } from "../../lib/capabilities";
import type { FacetSlot, ItemRecord, Link, Row, Slot, Tag } from "../../lib/types";
import { useTaskbarSlot } from "../../dock/TaskBar";
import { Thumbnail } from "../library/Thumbnail";

/** The four directions, in the order the cross draws them. The sense is kept
 * beside the name because "north" alone says nothing — and because N↔S invert
 * while W↔W and E↔E do not (G23), which is only legible if both ends are
 * named. */
const COMPASS: { key: string; name: string; sense: string }[] = [
  { key: "N", name: "North", sense: "broader" },
  { key: "W", name: "West", sense: "related" },
  { key: "E", name: "East", sense: "opposing" },
  { key: "S", name: "South", sense: "narrower" },
];

const SOURCE_LABEL: Record<string, string> = {
  local_file: "A file on disk",
  remote_url: "A web address",
  app_generated: "Made by Archiva",
};

const AVAILABILITY_NOTE: Record<string, string> = {
  present: "Reachable right now.",
  missing: "Not where it was last seen. Nothing has been deleted.",
  remote_uncached: "Not fetched yet — not broken, and not here.",
  permission_denied: "It is there, and this machine will not open it.",
};

function formatBytes(bytes: number | null): string | null {
  if (!bytes) return null;
  const mb = bytes / 1_048_576;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

/* --------------------------------------------------- the compass cross */

/** One arm of the cross: what this item points at in one direction.
 *
 * Drawn even when empty, and for the same reason an unfilled facet is drawn —
 * the empty arm is the prompt, and a cross missing an arm stops being a cross.
 * The list folds away because four arms of five entries each would push
 * everything below the compass off the pane. */
function CompassArm({
  slot,
  name,
  sense,
  area,
}: {
  slot: Slot;
  name: string;
  sense: string;
  area: string;
}) {
  const [open, setOpen] = useState(true);
  // The groups are by node type; the cross wants the arm as one list, and the
  // per-group cap is still respected by counting what it left out.
  const links = slot.groups.flatMap((g) => g.links);
  const hidden = slot.groups.reduce((n, g) => n + (g.total - g.links.length), 0);
  const empty = slot.total === 0;

  // `vacant`, not `empty`: `.empty` is already the pane-wide placeholder
  // ("nothing selected"), so borrowing its name gave every unfilled arm a
  // centring flex box with 64px of padding — a stretched, empty rectangle
  // where a one-line "none" belonged.
  return (
    <div className={`compass-arm ${area}${empty ? " vacant" : ""}`}>
      <button
        className="compass-head"
        disabled={empty}
        aria-expanded={!empty && open}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="compass-dir">{name}</span>
        <span className="compass-sense">{sense}</span>
        <span className="compass-count">
          {empty ? "none" : `${slot.total} item${slot.total === 1 ? "" : "s"}`}
        </span>
        {!empty && <span className="group-caret">{open ? "▾" : "▸"}</span>}
      </button>
      {!empty && open && (
        <ul className="compass-list">
          {links.map((l) => (
            <li key={l.edge_id} title={l.label ?? l.kind}>
              <span className="icon">
                <Thumbnail item={l.node} />
              </span>
              <span className="compass-name">{l.node.display_name}</span>
            </li>
          ))}
          {hidden > 0 && <li className="compass-more">+{hidden} more</li>}
        </ul>
      )}
    </div>
  );
}

/** The item at the centre, with what it points at around it. Four separate
 * sections could say the same things and could not say *this*: that the four
 * directions are one structure, and which of them this item has nothing in. */
function CompassCross({ slots, node }: { slots: Slot[]; node: Row }) {
  const at = (key: string): Slot =>
    slots.find((s) => s.compass === key) ?? { compass: key, total: 0, groups: [] };
  const total = slots.reduce((n, s) => n + s.total, 0);

  return (
    <section className="inspect-block">
      <h3>
        Compass
        <span className="count">{total}</span>
      </h3>
      <div className="compass-cross">
        {COMPASS.map((c) => (
          <CompassArm
            key={c.key}
            slot={at(c.key)}
            name={c.name}
            sense={c.sense}
            area={`at-${c.key.toLowerCase()}`}
          />
        ))}
        <div className="compass-centre">
          <span className="icon">
            <Thumbnail item={node} />
          </span>
          <span className="compass-centre-name">{node.display_name}</span>
        </div>
      </div>
      <p className="hint">
        North and South invert: what is broader than this has this as something
        narrower. West and East do not — related and opposing read the same from
        either end (G23).
      </p>
    </section>
  );
}

function LinkTile({ link }: { link: Link }) {
  return (
    <div className="link-tile" title={link.label ?? link.kind}>
      <span className="icon">
        <Thumbnail item={link.node} />
      </span>
      <span className="link-name">{link.node.display_name}</span>
    </div>
  );
}

/* ------------------------------------------------------- the facet grid */

/** One facet's row: the tags this item holds in it, and a way to add one.
 *
 * Every facet is drawn whether or not it is filled, because an empty slot is
 * the prompt — hiding it would hide the thing the view most needs to show. */
function FacetRow({
  slot,
  known,
  targets,
  busy,
  onApply,
  onRemove,
}: {
  slot: FacetSlot;
  known: Tag[];
  targets: string[];
  busy: boolean;
  onApply: (name: string, facet: string) => void;
  onRemove: (tagId: string) => void;
}) {
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const listId = `facet-${slot.facet}`;

  useEffect(() => {
    if (adding) inputRef.current?.focus();
  }, [adding]);

  const options = known.filter((t) => t.facet === slot.facet);

  return (
    <div className="facet-row">
      <div className="facet-head">
        <span className="facet-label" title={slot.hint}>
          {slot.label}
        </span>
        {slot.machineFillable && (
          <span className="facet-note" title="Archiva can propose values for this one">
            metadata
          </span>
        )}
      </div>
      <div className="facet-tags">
        {slot.tags.map((t) => (
          <span className="tag-chip" key={t.id}>
            {t.name}
            <button
              className="tag-x"
              title={`Remove from ${targets.length === 1 ? "this item" : `${targets.length} items`}`}
              disabled={busy}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => onRemove(t.id)}
            >
              ×
            </button>
          </span>
        ))}
        {adding ? (
          <form
            className="tag-add"
            onSubmit={(e) => {
              e.preventDefault();
              const name = draft.trim();
              if (name) onApply(name, slot.facet);
              setDraft("");
              setAdding(false);
            }}
          >
            <input
              ref={inputRef}
              value={draft}
              list={listId}
              placeholder={slot.hint}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={() => {
                setDraft("");
                setAdding(false);
              }}
              onKeyDown={(e) => {
                // Escape belongs to the field while it is open, or it closes
                // the pane behind it instead.
                if (e.key === "Escape") {
                  e.stopPropagation();
                  setDraft("");
                  setAdding(false);
                }
              }}
            />
            <datalist id={listId}>
              {options.map((t) => (
                <option key={t.id} value={t.name}>
                  {t.usage} item{t.usage === 1 ? "" : "s"}
                </option>
              ))}
            </datalist>
          </form>
        ) : (
          <button
            className="tag-plus"
            disabled={busy}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setAdding(true)}
          >
            +
          </button>
        )}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------ the view */

export function InspectorView({ isActive }: { isActive: boolean }) {
  const { id, selection } = useActiveItem();
  const [rec, setRec] = useState<ItemRecord | null>(null);
  const [known, setKnown] = useState<Tag[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const slot = useTaskbarSlot();

  // Tagging writes to the whole selection; everything else describes the one
  // active item. Falling back to the active id keeps a single click working
  // before any view has published a selection.
  const targets = useMemo(
    () => (selection.length > 0 ? selection : id ? [id] : []),
    [selection, id],
  );

  const load = useCallback(async () => {
    if (!id) {
      setRec(null);
      return;
    }
    try {
      const [r, tags] = await Promise.all([nodeRecord(id), listTags()]);
      setRec(r);
      setKnown(tags);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [id]);

  useEffect(() => {
    load();
  }, [load]);

  useArchivaChanged(load);

  /** Every write goes through here: one place that reports failure and one
   * that refreshes, rather than each button inventing both. */
  const write = useCallback(
    async (fn: () => Promise<unknown>) => {
      setBusy(true);
      try {
        await fn();
        setError(null);
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
      // The backend emits archiva:changed, which reloads this pane and every
      // other one. Reloading here as well would be a second refresh path.
    },
    [],
  );

  const onApply = (name: string, facet: string) =>
    write(async () => {
      const tagId = await createTag(name, facet);
      await applyTag(targets, tagId);
    });

  const onRemove = (tagId: string) => write(() => removeTag(targets, tagId));

  const controls = (
    <>
      <span className="taskbar-name">Inspector</span>
      <span className="taskbar-divider" />
      <span className="taskbar-status">{rec ? rec.identity.displayName : "Nothing selected"}</span>
      <span className="taskbar-spacer" />
      {rec && (
        <span className={`chip health-${rec.health.score}`} title={rec.health.description}>
          {rec.health.label}
        </span>
      )}
    </>
  );

  const body = () => {
    if (error && !rec) return <div className="empty"><span className="error">{error}</span></div>;
    if (!id || !rec) {
      return (
        <div className="empty">
          <div>Nothing selected.</div>
          <div className="hint">Pick an item in Library, Scattered or the Viewer and it appears here.</div>
        </div>
      );
    }

    const { identity, source, proxies, classification, health, history, node, attributes, slots, suggestions } = rec;
    const size = formatBytes(source.sizeBytes);

    return (
      <div className="inspect">
        <div className="inspect-head">
          <span className="inspect-thumb">
            <Thumbnail item={node} />
          </span>
          <div>
            <div className="inspect-title">{identity.displayName}</div>
            <div className="inspect-sub">{identity.displaySubtitle}</div>
          </div>
        </div>

        {error && <div className="inspect-error">{error}</div>}

        {/* ---------------------------------------------- classification */}

        <section className="inspect-block">
          <h3>
            Classification
            <span className="count">
              {health.facetsFilled}/{health.facetTarget}
            </span>
          </h3>
          {targets.length > 1 && (
            <p className="batch-note">
              Adding or removing a tag here applies to all {targets.length} selected items. Everything
              else on this page describes the one shown.
            </p>
          )}
          {/* The three tiers side by side rather than stacked: they are three
              kinds of question about one item — what it is, what it is about,
              what is in it — and reading them as columns is what makes an
              empty one visible at a glance. The header counts its own tier,
              so "which of these have I actually filled in" needs no arithmetic. */}
          <div className="tier-columns">
            {classification.tiers.map((tier) => (
              <div className="tier" key={tier.tier}>
                <div className="tier-head" title={`tier ${tier.tier}`}>
                  <span>{tier.label}</span>
                  <span className="tier-n">
                    {tier.facets.filter((f) => f.tags.length > 0).length}/{tier.facets.length}
                  </span>
                </div>
                {tier.facets.map((f) => (
                  <FacetRow
                    key={f.facet}
                    slot={f}
                    known={known}
                    targets={targets}
                    busy={busy}
                    onApply={onApply}
                    onRemove={onRemove}
                  />
                ))}
              </div>
            ))}
          </div>

          {classification.suggestions.length > 0 && (
            <div className="suggests">
              {/* Principle 3: the machine suggests, the user classifies.
                  Nothing here is ever applied automatically. */}
              <div className="suggests-head">Suggested — nothing is applied until you accept it</div>
              {classification.suggestions.map((s) => (
                <div className="suggest" key={s.key}>
                  <span className="suggest-what">
                    <b>{s.name}</b> as {s.facet}
                  </span>
                  <span className="suggest-why">from {s.evidence}</span>
                  <button
                    className="btn"
                    disabled={busy}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => write(() => acceptSuggestion(id, s.facet, s.name))}
                  >
                    Accept
                  </button>
                  <button
                    className="btn quiet"
                    title="Never offer this again"
                    disabled={busy}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => write(() => dismissSuggestion(s.key, "metadata_tag"))}
                  >
                    Dismiss
                  </button>
                </div>
              ))}
            </div>
          )}

          <div className="health-parts">
            <span className={`chip health-${health.score}`} title={health.description}>
              {health.label}
            </span>
            <span className="chip quiet">
              {health.facetsFilled} of {health.facetTarget} facets
            </span>
            <span className="chip quiet">
              {health.titleQuality ? "has its own title" : "filename as title"}
            </span>
            {health.unresolvedLinks > 0 && (
              <span className="chip warn">{health.unresolvedLinks} unresolved links</span>
            )}
          </div>
          <p className="hint">
            The parts are kept beside the score on purpose: one number cannot tell well-tagged-but-badly-named
            from well-named-but-untagged, and those need different prompts.
          </p>
        </section>

        {/* ---------------------------------------------------- identity */}

        <section className="inspect-block">
          <h3>Identity</h3>
          <dl>
            <dt>Id</dt>
            <dd className="mono wrap">{identity.id}</dd>
            <dt>Kind</dt>
            <dd>{identity.nodeType}</dd>
            <dt>Type</dt>
            <dd className="mono">{identity.contentType}</dd>
            <dt>Conforms to</dt>
            <dd>
              <span className="chain">
                {identity.conformsTo.map((t, i) => (
                  <span key={t}>
                    {i > 0 && <span className="chain-sep">›</span>}
                    <span className="chain-link mono">{t}</span>
                  </span>
                ))}
              </span>
            </dd>
            <dt>Added</dt>
            <dd>{identity.createdAt}</dd>
            <dt>Indexed</dt>
            <dd>{identity.indexedAt}</dd>
            <dt>Changed</dt>
            <dd>{identity.modifiedAt}</dd>
          </dl>
          <p className="hint">
            The id was minted once by the indexer and never changes. It is not the contents, not the path
            and not the name — all three of those are mutable, and the fingerprint below is an attribute
            beside the identity rather than the identity itself.
          </p>
        </section>

        {/* ------------------------------------------------------ source */}

        <section className="inspect-block">
          <h3>
            Source
            <span className="count">{SOURCE_LABEL[source.sourceKind] ?? source.sourceKind}</span>
          </h3>
          <dl>
            <dt>Availability</dt>
            <dd className={source.availability === "present" ? "" : "error"}>
              {source.availability.replace(/_/g, " ")}
              <div className="hint">{AVAILABILITY_NOTE[source.availability]}</div>
            </dd>
            {source.locator && (
              <>
                <dt>Where</dt>
                <dd className="mono wrap">{source.locator}</dd>
              </>
            )}
            {source.lastSeenAt && (
              <>
                <dt>Last seen</dt>
                <dd className="mono">{source.lastSeenAt}</dd>
              </>
            )}
            {size && (
              <>
                <dt>Size</dt>
                <dd>{size}</dd>
              </>
            )}
            {source.contentHash && (
              <>
                <dt>Fingerprint</dt>
                <dd className="mono wrap">{source.contentHash}</dd>
              </>
            )}
            {source.inode !== null && (
              <>
                <dt>Inode</dt>
                <dd className="mono">
                  {source.inode}
                  {source.device !== null && ` on device ${source.device}`}
                </dd>
              </>
            )}
            {source.mtime && (
              <>
                <dt>Modified</dt>
                <dd className="mono">{source.mtime}</dd>
              </>
            )}
          </dl>
          {source.sourceKind === "remote_url" && (
            <p className="hint">
              Fetching and caching remote content isn't built yet, so this stays “remote uncached”. That is
              the honest state for it — and visibly different from a file that has gone missing.
            </p>
          )}
        </section>

        {/* ----------------------------------------------------- proxies */}

        <section className="inspect-block">
          <h3>
            Proxies
            <span className="count">v{proxies.version}</span>
          </h3>
          <dl>
            <dt>State</dt>
            <dd>{proxies.state.replace(/_/g, " ")}</dd>
            <dt>Grid thumbnail</dt>
            <dd className="mono wrap">{proxies.thumbRef ?? "—"}</dd>
            <dt>Preview render</dt>
            <dd className="mono wrap">{proxies.previewRef ?? "—"}</dd>
            <dt>Playable copy</dt>
            <dd className="mono wrap">{proxies.playableRef ?? "—"}</dd>
            <dt>Original</dt>
            <dd>{proxies.originalAvailable ? "reachable" : "not reachable"}</dd>
          </dl>
          <p className="hint">
            Four artefacts, tracked separately. One field for all of them is why the old build's “has a
            thumbnail” filter really meant “has any proxy at all”.
          </p>
        </section>

        {/* ---------------------------------------------------- measured */}

        {Object.keys(attributes).length > 0 && (
          <section className="inspect-block">
            <h3>
              Measured
              <span className="count">{Object.keys(attributes).length}</span>
            </h3>
            <dl>
              {Object.entries(attributes).map(([k, v]) => (
                <div key={k} style={{ display: "contents" }}>
                  <dt>{k.replace(/_/g, " ")}</dt>
                  <dd className="mono wrap">{v}</dd>
                </div>
              ))}
            </dl>
            <p className="hint">
              Taken at index time. A measurement not taken then needs a full re-scan to add later, which is
              why the indexer extracts generously.
            </p>
          </section>
        )}

        {/* ------------------------------------------------------- links */}

        <CompassCross slots={slots} node={node} />

        {suggestions.length > 0 && (
          <section className="inspect-block">
            <h3>
              Suggested links <span className="count">{suggestions.length}</span>
            </h3>
            <div className="link-tiles">
              {suggestions.map((l) => (
                <LinkTile key={l.edge_id} link={l} />
              ))}
            </div>
            <p className="hint">Proposed, not asserted. Accepting one needs a write path that has no UI yet.</p>
          </section>
        )}

        {/* ---------------------------------------------------- indexing */}

        <section className="inspect-block">
          <h3>
            Indexing
            <span className="count">{history.length}</span>
          </h3>
          {history.length === 0 ? (
            <p className="hint">
              Nothing recorded. Rule 6 — seen and unchanged — is deliberately never written, so an item that
              has only ever been found where it was left shows no history at all.
            </p>
          ) : (
            <ol className="events">
              {history.map((e, i) => (
                <li key={`${e.at}-${i}`}>
                  <div className="event-head">
                    <span className="event-rule">rule {e.rule}</span>
                    <span className="event-label">{e.ruleLabel}</span>
                    <span className="event-at mono">{e.at}</span>
                  </div>
                  <div className="hint">{e.ruleNote}</div>
                  {e.signals.length > 0 && (
                    <div className="chips">
                      {e.signals.map((s) => (
                        <span className="chip quiet" key={s}>
                          {s}
                        </span>
                      ))}
                    </div>
                  )}
                </li>
              ))}
            </ol>
          )}
        </section>

        {/* ------------------------------------------------ capabilities */}

        <section className="inspect-block">
          <h3>Can do now</h3>
          <div className="chips">
            {node.capabilities.map((c) => (
              <span className="chip" key={c}>
                {CAPABILITY_LABEL[c as Capability] ?? c}
              </span>
            ))}
          </div>
          <p className="hint">
            Resolved from the type's grant and this item's current state — not stored, so an unplugged drive
            changes it without a reindex.
          </p>
        </section>
      </div>
    );
  };

  return (
    <div className="body">
      {body()}
      {isActive && slot && createPortal(controls, slot)}
    </div>
  );
}
