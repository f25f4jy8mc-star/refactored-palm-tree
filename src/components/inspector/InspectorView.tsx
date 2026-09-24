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
// Writing is limited to what has a named write path: applying and removing
// tags (C2), and putting things in and taking them out of the compass arms
// (S4, S11). Rename and set-attribute do not exist yet. A field that looks
// editable but silently discards what you type is worse than one that plainly
// isn't offered.

import { createPortal } from "react-dom";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  acceptSuggestion,
  addToArm,
  applyTag,
  createTag,
  dismissSuggestion,
  listTags,
  nodeRecord,
  removeTag,
  revealInFileManager,
  searchLibrary,
  unlinkEdge,
} from "../../lib/api";
import { useActiveItem } from "../../lib/activeItem";
import { droppedIds, isItemDrag, itemDrag } from "../../lib/drag";
import { useWorkbench } from "../../lib/workbench";
import { useArchivaChanged } from "../../lib/events";
import {
  CAPABILITY_LABEL,
  openDestinations,
  previewKind,
  type Capability,
  type Destination,
  type OpenOption,
} from "../../lib/capabilities";
import type { Added, FacetSlot, Hit, ItemRecord, Link, Row, Slot, Tag } from "../../lib/types";
import { useTaskbarSlot } from "../../dock/TaskBar";
import { Thumbnail } from "../library/Thumbnail";
import { PreviewStage } from "../preview/PreviewStage";

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

/** A batch that refused some of what it was given is reported as an error
 * line — the rest still went in, and the reason is what the backend said. */
async function report(p: Promise<Added>): Promise<void> {
  const added = await p;
  if (added.refused.length > 0) {
    throw new Error(added.refused.map(([, why]) => why).join("; "));
  }
}

function formatBytes(bytes: number | null): string | null {
  if (!bytes) return null;
  const mb = bytes / 1_048_576;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

/* ----------------------------------------------------- open in… (menu) */

type MenuAt = { x: number; y: number; node: Row };

/** The right-click menu on a compass entry. What it offers comes from the
 * item's resolved capabilities, not from this file knowing what a photograph
 * is — see `openDestinations`. */
function OpenInMenu({
  at,
  onPick,
  onClose,
}: {
  at: MenuAt;
  onPick: (destination: Destination, node: Row) => void;
  onClose: () => void;
}) {
  const options: OpenOption[] = openDestinations(at.node);
  const box = useRef<HTMLDivElement>(null);

  // A click outside, a deliberate scroll, a resize, or Escape.
  //
  // "Outside" is asked of the element, not of the event's phase: the listener
  // is in capture so a click that would also select something behind the menu
  // closes it first, and capture runs *before* React's own handlers — so a
  // `stopPropagation` on the menu could not have saved it. Without the
  // containment check the menu unmounted on mousedown and the click never
  // reached the item, which looked exactly like a button that did nothing.
  //
  // `wheel`, not `scroll`: the menu is fixed and anchored to where the
  // pointer was, so a scroll does not carry it away from anything — and a
  // `scroll` listener also fires for scrolling nothing asked for. The pane
  // behind settling its own layout was closing the menu half a second after
  // it opened, which reads as a menu that will not stay up.
  useEffect(() => {
    const away = (e: Event) => {
      if (box.current?.contains(e.target as Node)) return;
      onClose();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("mousedown", away, true);
    window.addEventListener("wheel", away, true);
    window.addEventListener("resize", onClose);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("mousedown", away, true);
      window.removeEventListener("wheel", away, true);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("keydown", key, true);
    };
  }, [onClose]);

  return createPortal(
    <div
      ref={box}
      className="ctx-menu"
      style={{ left: at.x, top: at.y }}
      role="menu"
      onContextMenu={(e) => e.preventDefault()}
    >
      <div className="ctx-title">{at.node.display_name}</div>
      {options.length === 0 ? (
        <div className="ctx-none">Nothing can open this yet.</div>
      ) : (
        options.map((o) => (
          <button
            key={o.destination}
            className="ctx-item"
            role="menuitem"
            disabled={!o.enabled}
            onClick={() => {
              onPick(o.destination, at.node);
              onClose();
            }}
          >
            <span>Open in {o.label}</span>
            <span className="ctx-note">{o.note}</span>
          </button>
        ))
      )}
    </div>,
    document.body,
  );
}

/* --------------------------------------------------- the compass cross */

/** One arm of the cross: what this item points at in one direction.
 *
 * Drawn even when empty, and for the same reason an unfilled facet is drawn —
 * the empty arm is the prompt, and a cross missing an arm stops being a cross.
 * The list folds away because four arms of five entries each would push
 * everything below the compass off the pane. */
/** What an arm's kind of edge is called, so an entry can say what removing
 * it takes away — "untag", not "unlink", for a tag in North. */
const EDGE_NOUN: Record<string, string> = {
  tag_of: "tag",
  contains: "membership",
  wikilink: "wiki link",
  embed: "embed",
};

/** The search behind an arm's "+": the library's own search (p_search), so
 * what you can find to link is exactly what you can find anywhere else. */
function ArmPicker({
  item,
  onPick,
  onClose,
}: {
  item: string;
  onPick: (ids: string[]) => void;
  onClose: () => void;
}) {
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<Hit[]>([]);
  const [idx, setIdx] = useState(0);

  useEffect(() => {
    let live = true;
    const query = q.trim();
    if (!query) {
      setHits([]);
      return;
    }
    searchLibrary(query)
      .then((h) => {
        if (!live) return;
        // Linking an item to itself is refused by the write path; offering
        // it here would be offering the refusal.
        setHits(h.filter((x) => x.node.id !== item).slice(0, 8));
        setIdx(0);
      })
      .catch(() => live && setHits([]));
    return () => {
      live = false;
    };
  }, [q, item]);

  return (
    <div className="arm-picker">
      <input
        autoFocus
        value={q}
        placeholder="Find something to put here…"
        onChange={(e) => setQ(e.target.value)}
        onBlur={onClose}
        onKeyDown={(e) => {
          // The field owns its keys while it is open: Escape closes it rather
          // than the pane, arrows move in the results rather than the list.
          e.stopPropagation();
          if (e.key === "Escape") onClose();
          if (e.key === "ArrowDown") setIdx((i) => Math.min(i + 1, hits.length - 1));
          if (e.key === "ArrowUp") setIdx((i) => Math.max(i - 1, 0));
          if (e.key === "Enter" && hits[idx]) {
            e.preventDefault();
            onPick([hits[idx].node.id]);
          }
        }}
      />
      {hits.length > 0 && (
        <ul className="arm-hits">
          {hits.map((h, i) => (
            <li
              key={h.node.id}
              className={i === idx ? "on" : ""}
              onMouseDown={(e) => e.preventDefault()}
              onMouseEnter={() => setIdx(i)}
              onClick={() => onPick([h.node.id])}
            >
              <span className="icon">
                <Thumbnail item={h.node} />
              </span>
              <span className="compass-name">{h.node.display_name}</span>
              <span className="arm-hit-type">{h.node.node_type}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function CompassArm({
  slot,
  dir,
  name,
  sense,
  area,
  item,
  busy,
  onContext,
  onAdd,
  onUnlink,
}: {
  slot: Slot;
  dir: string;
  name: string;
  sense: string;
  area: string;
  item: string;
  busy: boolean;
  onContext: (e: React.MouseEvent, node: Row) => void;
  onAdd: (dir: string, ids: string[]) => void;
  onUnlink: (link: Link) => void;
}) {
  const { tray } = useWorkbench();
  const [open, setOpen] = useState(true);
  const [picking, setPicking] = useState(false);
  const [over, setOver] = useState(false);
  // The groups are by node type; the cross wants the arm as one list, and the
  // per-group cap is still respected by counting what it left out.
  const links = slot.groups.flatMap((g) => g.links);
  const hidden = slot.groups.reduce((n, g) => n + (g.total - g.links.length), 0);
  const empty = slot.total === 0;
  const fromTray = tray.filter((id) => id !== item);

  // `vacant`, not `empty`: `.empty` is already the pane-wide placeholder
  // ("nothing selected"), so borrowing its name gave every unfilled arm a
  // centring flex box with 64px of padding — a stretched, empty rectangle
  // where a one-line "none" belonged.
  //
  // The whole arm is a drop zone. Where something lands decides what it
  // becomes (S6) — a tag dropped in North is a tagging — and that decision is
  // the backend's; this only says which arm.
  return (
    <div
      className={`compass-arm ${area}${empty ? " vacant" : ""}${over ? " over" : ""}`}
      data-dir={dir}
      onDragOver={(e) => {
        if (!isItemDrag(e) || busy) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "link";
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        setOver(false);
        const ids = droppedIds(e).filter((id) => id !== item);
        if (ids.length === 0) return;
        e.preventDefault();
        onAdd(dir, ids);
      }}
    >
      <div className="compass-headrow">
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
        <button
          className="compass-add"
          title={`Put something in ${name} — or drop it here`}
          disabled={busy}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => setPicking((p) => !p)}
        >
          +
        </button>
      </div>
      {picking && (
        <>
          <ArmPicker
            item={item}
            onClose={() => setPicking(false)}
            onPick={(ids) => {
              setPicking(false);
              setOpen(true);
              onAdd(dir, ids);
            }}
          />
          {fromTray.length > 0 && (
            <button
              className="btn arm-from-tray"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                setPicking(false);
                setOpen(true);
                onAdd(dir, fromTray);
              }}
            >
              From the tray ({fromTray.length})
            </button>
          )}
        </>
      )}
      {!empty && open && (
        <ul className="compass-list">
          {links.map((l) => (
            <li
              key={l.edge_id}
              title={l.label ?? l.kind}
              onContextMenu={(e) => onContext(e, l.node)}
              {...itemDrag(() => [l.node.id])}
            >
              <span className="icon">
                <Thumbnail item={l.node} />
              </span>
              <span className="compass-name">{l.node.display_name}</span>
              <button
                className="compass-x"
                title={`Remove this ${EDGE_NOUN[l.kind] ?? "link"} — ${l.node.display_name} itself is not touched`}
                disabled={busy}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => onUnlink(l)}
              >
                ×
              </button>
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
function CompassCross({
  slots,
  node,
  busy,
  onOpen,
  onAdd,
  onUnlink,
}: {
  slots: Slot[];
  node: Row;
  busy: boolean;
  onOpen: (destination: Destination, node: Row) => void;
  onAdd: (dir: string, ids: string[]) => void;
  onUnlink: (link: Link) => void;
}) {
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const onContext = (e: React.MouseEvent, far: Row) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, node: far });
  };
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
            dir={c.key}
            name={c.name}
            sense={c.sense}
            area={`at-${c.key.toLowerCase()}`}
            item={node.id}
            busy={busy}
            onContext={onContext}
            onAdd={onAdd}
            onUnlink={onUnlink}
          />
        ))}
        <div className="compass-centre" onContextMenu={(e) => onContext(e, node)}>
          <span className="icon">
            <Thumbnail item={node} />
          </span>
          <span className="compass-centre-name">{node.display_name}</span>
        </div>
      </div>
      {menu && <OpenInMenu at={menu} onPick={onOpen} onClose={() => setMenu(null)} />}
      <p className="hint">
        North and South invert: what is broader than this has this as something
        narrower. West and East do not — related and opposing read the same from
        either end (G23). Add with an arm's +, or drop rows and tray items on it —
        a tag put in North tags this, a collector put in North holds it. Right-click
        an entry to open it elsewhere.
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

type Props = {
  isActive: boolean;
  /** Open something somewhere else. The Inspector decides *what* is
   * applicable (from resolved capabilities); the shell owns the panes and
   * decides how one gets opened. */
  onOpen?: (destination: Destination, node: Row) => void;
};

export function InspectorView({ isActive, onOpen }: Props) {
  const { id, selection, revealItem } = useActiveItem();
  const [rec, setRec] = useState<ItemRecord | null>(null);
  const [known, setKnown] = useState<Tag[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The head's preview, open to the width of the pane. On by default: the
  // first question about an item is usually what it looks like, and the
  // record below is reference you go to second.
  const [expanded, setExpanded] = useState(true);
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

  // A new item is a new decision, and the default is open — so a pane you
  // collapsed does not stay collapsed for everything you look at after.
  useEffect(() => setExpanded(true), [id]);

  /** Open a compass entry somewhere else.
   *
   * Two halves, because they are two different things. Revealing is a *list*
   * asking to go to a row, and the Inspector can say that directly — it is
   * the same channel the active item travels on. Bringing a pane forward, or
   * making one, is the shell's business, because only the shell owns panes.
   * A destination with no shell to ask is reported rather than swallowed. */
  const openSomewhere = useCallback(
    (destination: Destination, node: Row) => {
      if (destination === "library") revealItem(node.id);
      if (onOpen) {
        onOpen(destination, node);
      } else if (destination !== "library") {
        setError(`Nothing here can open “${node.display_name}” in the ${destination}.`);
      }
    },
    [onOpen, revealItem],
  );

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

    // What can be drawn at all, and therefore whether the head has a preview
    // to open out. `previewKind` reads the registry's own answer — the same
    // one a double-click follows — so this pane and Quick Look can never
    // disagree about which files can be shown.
    // `none` means there is no renderer for this — a collector, or a file
    // that is not reachable. Opening a placeholder out to the width of the
    // panel would be a large way of saying nothing.
    const canExpand = previewKind(node) !== "none";
    const open = expanded && canExpand;
    const stage = {
      node,
      locator: source.locator,
      previewRef: proxies.previewRef,
      thumbRef: proxies.thumbRef,
      playableRef: proxies.playableRef,
    };

    return (
      <div className="inspect">
        {/* One element, two sizes. Collapsing does not swap the preview for a
            thumbnail — it makes the same box 56px, and the still inside it
            shrinks. Open by default, because the first question about an item
            is usually what it looks like. */}
        <div className={"inspect-head" + (open ? " expanded" : "")}>
          <button
            className="inspect-shot"
            title={open ? "Collapse preview" : "Expand preview to the panel"}
            aria-expanded={open}
            disabled={!canExpand}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setExpanded((v) => !v)}
          >
            <PreviewStage item={stage} compact={!open} />
          </button>
          <div className="inspect-headings">
            <div className="inspect-title">{identity.displayName}</div>
            <div className="inspect-sub">{identity.displaySubtitle}</div>
          </div>
          {canExpand && (
            <button
              className={"btn" + (open ? " on" : "")}
              title={open ? "Collapse preview" : "Expand preview to the panel"}
              aria-expanded={open}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => setExpanded((v) => !v)}
            >
              ⤢
            </button>
          )}
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
                    onClick={() => write(() => dismissSuggestion(s.key, s.kind))}
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

        {/* ----------------------------------------------------- compass */}

        {/* Above Identity, because it is the question you are most often here
            to answer: what is this next to. The record below it — ids, paths,
            fingerprints — is reference, and reference belongs under the thing
            it is reference for. */}
        <CompassCross
          slots={slots}
          node={node}
          busy={busy}
          onOpen={openSomewhere}
          onAdd={(dir, ids) => write(() => report(addToArm(node.id, dir, ids)))}
          onUnlink={(l) => write(() => unlinkEdge(l.edge_id))}
        />

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
            {/* `reveal` is the registry's answer to "is there a place on this
                machine to open": a local file that is present. A derived
                folder Collector is not granted it, and neither is a file that
                has gone missing — so this button is never offered for
                something the file manager could not find. */}
            {node.capabilities.includes("reveal") && source.locator && (
              <button
                className="btn"
                title={`Show ${source.filename ?? identity.displayName} in the file manager`}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() =>
                  revealInFileManager(source.locator as string).catch((e) =>
                    setError(String(e)),
                  )
                }
              >
                Open item location
              </button>
            )}
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
