// Spaces: which library this window is looking at, and where it lives.
//
// A space is a folder you chose. Everything Archiva makes for that library —
// the index, the proxies, the notes written here — is inside it, which is
// what makes "your data on your disk" true rather than claimed: you back a
// space up by copying a folder, and you move it by moving one.
//
// This panel sits above the watched folders because that is the containing
// relationship: the folders are linked *into* a space, and switching space
// swaps the whole list.

import { useCallback, useEffect, useState } from "react";

import {
  createSpace,
  forgetSpace,
  listSpaces,
  moveSpace,
  openSpace,
  openSpaceFolder,
  pickFolder,
  renameSpace,
} from "../../lib/api";
import { useArchivaChanged } from "../../lib/events";
import type { Space } from "../../lib/types";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const mb = bytes / 1_048_576;
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

/** What a space holds, in the words of what made it. Zero is left out: a
 * space with no proxies yet should not read as a list of nothings. */
function holdings(space: Space): string {
  const parts = Object.entries(space.holds)
    .filter(([, bytes]) => bytes > 0)
    .map(([what, bytes]) => `${what} ${formatBytes(bytes)}`);
  return parts.length ? parts.join(" · ") : "nothing yet";
}

export function SpacesPanel({ onBusy }: { onBusy?: (busy: boolean) => void }) {
  const [spaces, setSpaces] = useState<Space[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [forgetting, setForgetting] = useState<Space | null>(null);

  const load = useCallback(async () => {
    try {
      setSpaces(await listSpaces());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  useArchivaChanged(load);

  /** Every write goes through here: one place that reports failure and one
   * that refreshes, rather than each button inventing both. */
  const run = useCallback(
    async (fn: () => Promise<unknown>) => {
      setWorking(true);
      onBusy?.(true);
      try {
        await fn();
        setError(null);
      } catch (e) {
        setError(String(e));
      } finally {
        setWorking(false);
        onBusy?.(false);
        load();
      }
    },
    [load, onBusy],
  );

  const current = spaces.find((s) => s.isCurrent) ?? null;
  const others = spaces.filter((s) => !s.isCurrent);

  return (
    <div className="spaces">
      <div className="flyout-head">
        <h2>Space</h2>
        <span className="hint">The folder this library lives in</span>
      </div>

      {error && <div className="flyout-error">{error}</div>}

      {current ? (
        <div className="space-current">
          {renaming === current.id ? (
            <form
              className="space-rename"
              onSubmit={(e) => {
                e.preventDefault();
                const name = draft.trim();
                setRenaming(null);
                if (name && name !== current.name) run(() => renameSpace(current.id, name));
              }}
            >
              <input
                autoFocus
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onBlur={() => setRenaming(null)}
                onKeyDown={(e) => e.key === "Escape" && setRenaming(null)}
              />
            </form>
          ) : (
            <button
              className="space-name"
              title="Rename this space"
              disabled={working}
              onClick={() => {
                setDraft(current.name);
                setRenaming(current.id);
              }}
            >
              {current.name}
            </button>
          )}
          <div className="space-root mono">{current.root}</div>
          <div className="space-holds">{holdings(current)}</div>
          <div className="space-actions">
            <button
              className="btn"
              disabled={working}
              title="Move this space's folder, with everything in it"
              onClick={async () => {
                const to = await pickFolder();
                if (to) run(() => moveSpace(current.id, to));
              }}
            >
              Move…
            </button>
          </div>
        </div>
      ) : (
        <p className="hint">
          No space is open. Make one, or open a folder that already holds one.
        </p>
      )}

      {others.length > 0 && (
        <ul className="space-list">
          {others.map((s) => (
            <li key={s.id} className={s.reachable ? "" : "away"}>
              <button
                className="space-switch"
                disabled={working || !s.reachable}
                title={s.reachable ? `Open ${s.name}` : "This folder isn’t there right now"}
                onClick={() => run(() => openSpace(s.id))}
              >
                <span className="space-switch-name">{s.name}</span>
                <span className="space-root mono">{s.root}</span>
              </button>
              <button
                className="btn quiet"
                title="Stop listing this space here. The folder is never deleted."
                disabled={working}
                onClick={() => setForgetting(s)}
              >
                ✕
              </button>
            </li>
          ))}
        </ul>
      )}

      <div className="space-actions">
        <button
          className="btn"
          disabled={working}
          onClick={async () => {
            const at = await pickFolder();
            if (!at) return;
            const name = at.split("/").filter(Boolean).pop() ?? "My Archive";
            run(() => createSpace(name, at));
          }}
        >
          New space…
        </button>
        <button
          className="btn"
          disabled={working}
          title="A folder that already holds a space — a backup drive, another machine"
          onClick={async () => {
            const at = await pickFolder();
            if (at) run(() => openSpaceFolder(at));
          }}
        >
          Open a space…
        </button>
      </div>
      <p className="hint">
        A new space wants a folder of its own. Everything Archiva makes for this
        library goes there — the index, proxies and notes — so the whole library
        is one folder you can copy, move or back up.
      </p>

      {forgetting && (
        <div className="confirm">
          <div className="confirm-what">
            Stop listing <b>{forgetting.name}</b> here?
          </div>
          <p className="hint">
            Nothing is deleted. {forgetting.root} stays exactly as it is, and
            opening that folder again brings the whole library back.
          </p>
          <div className="confirm-actions">
            <button className="btn" onClick={() => setForgetting(null)}>
              Cancel
            </button>
            <button
              className="btn primary"
              onClick={() => {
                const id = forgetting.id;
                setForgetting(null);
                run(() => forgetSpace(id));
              }}
            >
              Stop listing it
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

/** The first run, and the "that drive isn't plugged in" run. Not an error
 * screen: neither state means anything is wrong, and both are fixed by the
 * same two buttons. */
export function NoSpace() {
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);

  const run = async (fn: () => Promise<unknown>) => {
    setWorking(true);
    try {
      await fn();
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setWorking(false);
    }
  };

  return (
    <div className="no-space">
      <div className="no-space-card">
        <h1>Archiva</h1>
        <p>
          A space is a folder on your disk that holds one library: its index, its
          proxies, the notes you write here, and the list of folders you link
          into it. Nothing is copied into it that you did not put there.
        </p>
        {error && <div className="flyout-error">{error}</div>}
        <div className="space-actions">
          <button
            className="btn primary"
            disabled={working}
            onClick={async () => {
              const at = await pickFolder();
              if (!at) return;
              const name = at.split("/").filter(Boolean).pop() ?? "My Archive";
              run(() => createSpace(name, at));
            }}
          >
            New space…
          </button>
          <button
            className="btn"
            disabled={working}
            onClick={async () => {
              const at = await pickFolder();
              if (at) run(() => openSpaceFolder(at));
            }}
          >
            Open a space…
          </button>
        </div>
        <p className="hint">
          Pick an empty folder for a new space. To open one you already have —
          on a backup drive, or from another machine — point at the folder
          itself.
        </p>
      </div>
    </div>
  );
}
