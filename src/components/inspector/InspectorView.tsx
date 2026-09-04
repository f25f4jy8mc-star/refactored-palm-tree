// The Inspector: everything `p_detail` knows about one node, for whatever
// is currently active. It follows the active item rather than owning a
// selection of its own — that was a real bug in the old build, where a
// viewer pane overwrote the inspector's item on every focus change.
//
// Read-only for now, and deliberately so: editing a title, adding a tag or
// dropping a link all need mutations that exist in `model/mutations.rs`
// (link/unlink/reorder) or don't exist yet (rename, set-attribute), and a
// field that looks editable but silently discards what you type is worse
// than one that plainly doesn't.

import { createPortal } from "react-dom";
import { useCallback, useEffect, useState } from "react";

import { nodeDetail } from "../../lib/api";
import { useActiveItem } from "../../lib/activeItem";
import { useArchivaChanged } from "../../lib/events";
import { CAPABILITY_LABEL, type Capability } from "../../lib/capabilities";
import type { Detail, Link } from "../../lib/types";
import { useTaskbarSlot } from "../../dock/TaskBar";
import { Thumbnail } from "../library/Thumbnail";

const COMPASS_LABEL: Record<string, string> = {
  N: "North — broader",
  S: "South — narrower",
  W: "West — related",
  E: "East — opposing",
};

function formatBytes(bytes: number | null): string | null {
  if (!bytes) return null;
  const mb = bytes / 1_048_576;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
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

export function InspectorView({ isActive }: { isActive: boolean }) {
  const { id } = useActiveItem();
  const [detail, setDetail] = useState<Detail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const slot = useTaskbarSlot();

  const load = useCallback(async () => {
    if (!id) {
      setDetail(null);
      return;
    }
    try {
      setDetail(await nodeDetail(id));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [id]);

  useEffect(() => {
    load();
  }, [load]);

  useArchivaChanged(load);

  const controls = (
    <>
      <span className="taskbar-name">Inspector</span>
      <span className="taskbar-divider" />
      <span className="taskbar-status">
        {detail ? detail.node.display_name : "Nothing selected"}
      </span>
      <span className="taskbar-spacer" />
    </>
  );

  const body = () => {
    if (error) return <div className="empty"><span className="error">{error}</span></div>;
    if (!id || !detail) {
      return (
        <div className="empty">
          <div>Nothing selected.</div>
          <div className="hint">Pick an item in Library, Scattered or the Viewer and it appears here.</div>
        </div>
      );
    }

    const { node, attributes, slots, suggestions } = detail;
    const size = formatBytes(detail.sizeBytes);

    return (
      <div className="inspect">
        <div className="inspect-head">
          <span className="inspect-thumb">
            <Thumbnail item={node} />
          </span>
          <div>
            <div className="inspect-title">{node.display_name}</div>
            <div className="inspect-sub">{node.display_subtitle}</div>
          </div>
        </div>

        <section className="inspect-block">
          <h3>Identity</h3>
          <dl>
            <dt>Type</dt>
            <dd className="mono">{node.content_type}</dd>
            <dt>Kind</dt>
            <dd>{node.node_type}</dd>
            <dt>Availability</dt>
            <dd className={node.availability === "present" ? "" : "error"}>
              {node.availability.replace("_", " ")}
            </dd>
            {size && (
              <>
                <dt>Size</dt>
                <dd>{size}</dd>
              </>
            )}
            <dt>Proxy</dt>
            <dd>{node.proxy_state.replace("_", " ")}</dd>
            {detail.locator && (
              <>
                <dt>Where</dt>
                <dd className="mono wrap">{detail.locator}</dd>
              </>
            )}
            <dt>Id</dt>
            <dd className="mono wrap">{node.id}</dd>
          </dl>
        </section>

        {Object.keys(attributes).length > 0 && (
          <section className="inspect-block">
            <h3>Measured</h3>
            <dl>
              {Object.entries(attributes).map(([k, v]) => (
                <div key={k} style={{ display: "contents" }}>
                  <dt>{k.replace(/_/g, " ")}</dt>
                  <dd className="mono wrap">{v}</dd>
                </div>
              ))}
            </dl>
          </section>
        )}

        {slots.map((slot) =>
          slot.total === 0 ? null : (
            <section className="inspect-block" key={slot.compass}>
              <h3>
                {COMPASS_LABEL[slot.compass] ?? slot.compass}
                <span className="count">{slot.total}</span>
              </h3>
              {slot.groups.map((g) => (
                <div key={g.node_type} className="link-group">
                  <div className="link-group-head">
                    {g.node_type}
                    {g.total > g.links.length && <span className="count">+{g.total - g.links.length}</span>}
                  </div>
                  <div className="link-tiles">
                    {g.links.map((l) => (
                      <LinkTile key={l.edge_id} link={l} />
                    ))}
                  </div>
                </div>
              ))}
            </section>
          ),
        )}

        {suggestions.length > 0 && (
          <section className="inspect-block">
            {/* Principle 3: the machine suggests, the user classifies. These
                are visibly separate from anything asserted, and accepting
                one needs mutations::accept, which has no UI yet. */}
            <h3>
              Suggested <span className="count">{suggestions.length}</span>
            </h3>
            <div className="link-tiles">
              {suggestions.map((l) => (
                <LinkTile key={l.edge_id} link={l} />
              ))}
            </div>
          </section>
        )}

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
            Resolved from the type's grant and this item's current state — not stored, so an
            unplugged drive changes it without a reindex.
          </p>
        </section>

        {detail.unresolved_links > 0 && (
          <section className="inspect-block">
            <h3>
              Unresolved links <span className="count">{detail.unresolved_links}</span>
            </h3>
            <p className="hint">Wikilinks pointing at a name nothing carries yet.</p>
          </section>
        )}
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
