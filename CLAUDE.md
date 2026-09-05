# Archiva — working notes

Self-hosted visual archive and idea-development tool. Tauri v2 + Rust behind
React + TypeScript.

## Check every change against the checklist

`docs/archiva-requirements-checklist.md` is the specification. **Read it
before starting any tweak or new build, and check the finished work back
against it.** It is the compiled record of every prior decision, with the
reasoning attached, so a detail cannot quietly be reinterpreted.

Two companions to the same data:

- `docs/archiva-requirements.html` — the same 73 requirements as an
  interactive checklist. Filter by state; select a line for the reasoning.
- `docs/archiva-schema-map.html` — the eleven tables, drawn, with what each
  holds and why it is shaped that way.

Every requirement carries an id (`I9`, `C7`, `S11`…). Use those ids in commit
messages and in comments, so a piece of code can be traced back to the
sentence that asked for it.

The four states in the checklist mean different things:

| | Meaning |
|---|---|
| `[x]` **tested** | Written and tested. **Carry across unchanged — do not rewrite.** |
| `[ ]` **rebuild** | Worked in Build 17. Behaviour proven, code deliberately left behind. Fresh implementation against the Phase 0 spec. |
| `[ ]` **unbuilt** | Specified in Phase 0, no code exists. |
| `[?]` **decide** | Waiting on the user. Do not pick one silently — say which way you went and why. |

## The two rules everything follows

1. **If two views could disagree about something, it belongs in the model,
   not in a view.** Hence the projections and the one capability registry.
2. **Identity is minted, never derived.** A node's id comes from the indexer
   and never changes. Not the contents, not the path, not the name.

## What must not be rewritten

`src-tauri/src/model/{backfill,capabilities,content_type,extract,mutations,projections,reconcile,scan,signals}.rs`
and `migrations_model/001_model.sql` were delivered tested. Additive changes
only — new modules alongside them, never edits to their logic.

Anything reading those modules is bound by what they already decide. For
example `projections::to_row` phrases health as "N of 3 facets" and treats
`title_quality == 0` as "filename as title", so `health.rs` computes to that
scale rather than inventing its own.

## Verification before any status report

`cargo test` and `npm run build` both pass before anything is described as
working. This is checklist P13 and the single biggest process lesson from the
rebuild. Never report a feature as done off a successful compile alone.

For interface work there is a headless walkthrough harness: serve `dist/` and
stub `window.__TAURI_INTERNALS__`, then drive the real frontend with Playwright
(Chromium at `/opt/pw-browsers/chromium`, packages under
`/opt/node22/lib/node_modules`).

**A hand-written stub proves nothing about the backend.** The tree work passed
its walkthrough while the app was broken, because the stub answered with rows
someone had written by hand and the Rust answered with something else. Where a
walkthrough depends on the shape of backend data, generate a fixture from the
real thing — `ARCHIVA_FIXTURE=<path> cargo test emit_fixture` scans a real
directory tree and dumps what the real projections return — and drive the
harness from that. If the frontend asks for something the fixture has no
recorded answer for, that is a disagreement to surface, not a gap to fill in.

Frontend logic that can be pulled out into a pure module gets a vitest test
instead — see `src/lib/selection.ts`, `placement.ts`, `expansion.ts`.

## Layout

```
src-tauri/src/model/     the model: projections, capabilities, the ladder
src-tauri/src/commands.rs  the Tauri surface — thin, no query logic
src/lib/                 shared frontend logic (selection, shortcuts, api)
src/components/<view>/   one directory per view
src/dock/                the shell: rail, taskbar, dockview panes
```

Commands are deliberately thin. Query logic in `commands.rs` would be a
second copy of what a projection already decides.
