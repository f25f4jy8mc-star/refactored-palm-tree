# Archiva — requirements checklist
### Self-hosted visual archive and idea-development tool for creators and archivists

Compiled 2026-09-04 from every prior session in this project. **73 requirements** across 8 areas.

Sorted by what still has to happen to each item, not by what it does. The point of this document is that you can hand it to a build tool and it will not have to guess which parts are settled.

> A companion file, `archiva-requirements.html`, holds the same list as an interactive checklist — filter by state, select a line for the reasoning. It reads from the same data, so the two cannot disagree.

---

## The four states

| | State | Count | What it means |
|---|---|---|---|
| `[x]` | **tested** | 9 | Written and tested — carry across unchanged. Rust modules under src-tauri/src/model/ plus migrations_model/001_model.sql. Not to be rewritten by the new build. |
| `[ ]` | **rebuild** | 31 | Worked in Build 17 — must be rebuilt on the new model. The behaviour is proven and the code is not. Build 17 is being left behind, so each of these is a fresh implementation against the Phase 0 spec. |
| `[ ]` | **unbuilt** | 28 | Specified in Phase 0 — never built. Defined field by field in archiva-phase-0-model.md. No code exists. |
| `[?]` | **decide** | 5 | Undecided — needs your call. Deliberately left open, usually because it needs a real library to answer. |

Read the headline as: **9 of 73 requirements are already written and tested**, 31 are proven behaviour waiting to be reimplemented, 28 are specified on paper but have no code at all, and 5 are waiting on a decision from you.

The `rebuild` column is the one that misleads if you skim it. Those features worked in Build 17. The behaviour is proven; the code is being left behind deliberately, so each one is a fresh implementation against the new model.

---

## The checklist

### Identity and indexing · 13

*How a file on disk becomes a row in the database, and stays the same row when it moves.*

- `[x]` **UUID v7 as node identity** — *tested*  
  Every node gets an identifier minted once by the indexer, which never changes and is never derived from the file's contents. UUID v7 sorts by creation time, so ordering by id is a free stable tiebreaker.  
  › Build 17 used the content hash as identity. Editing a note changed its bytes, which changed its hash, which created a second node — the duplicate-record bug class.
- `[x]` **BLAKE3 hash demoted to an attribute** — *tested*  
  content_hash is stored for deduplication, integrity and change detection, and is nullable. The index on it is deliberately not unique.  
  › Two files may legitimately hold identical bytes. The unique index in Build 17 is the direct cause of the duplicate-node bug.
- `[x]` **Reconciliation ladder — thirteen rules** — *tested*  
  Given a file, decide what happened to it: new, unchanged, edited in place, renamed, moved to another volume, copied, restored from backup, saved by an app, mid-download, unreadable. Rules are read in order and the first match wins.  
  › Verified before it was written: all 512 signal combinations swept with no holes, and thirteen named real-world scenarios each landing on the intended rule. Those assertions are now Rust tests.
- `[x]` **Move detection via inode, then hash, then path** — *tested*  
  Identity survives an external move. Notes carry their UUID in frontmatter as archiva-id. Media cannot, so the ladder tries (inode, device), then content_hash, then (filename, size, mtime).  
  › Files get renamed and reorganised outside the app. Path-based identity breaks every link when that happens.
- `[x]` **Filesystem walk and scan** — *tested*  
  scan.rs walks the watch roots, gathers signals per file, and applies the ladder. A second scan over an unchanged library must be entirely rule 6 with nothing created, nothing updated, and no new log rows.  
  › That idempotence check is the test that catches a rule firing when it shouldn't, and the rule number names which one.
- `[x]` **Content-type classification and conformance closure** — *tested*  
  Each item gets a leaf content_type in UTI style, such as public.jpeg, plus content_type_tree — the full list of types it conforms to, stored leaf-first.  
  › This is what lets a filter mean 'anything that is an image' and pick up HEIC and Photoshop files without anyone editing a list.
- `[x]` **Build 17 backfill** — *tested*  
  backfill.rs converts an existing v1 library into the new schema in one transaction: splits every old North link into tag_of, contains or compass_n; recomputes tags stored at the wrong tier; unpacks metadata JSON into sortable rows; preserves missing flags.  
  › Run it before the first scan of the new database. Scanning first creates nodes at the same paths, the unique locator index rejects the backfill, and the whole thing aborts.
- `[ ]` **Watch folders — multiple, individually enabled** — *rebuild*  
  The user nominates folders to index. Each can be switched off without losing its items.  
  › Worked in Build 17 behind the Sources flyout. Behaviour proven, code not carried.
- `[ ]` **Availability as four states** — *unbuilt*  
  present, missing, remote_uncached, permission_denied — with last_seen_at alongside.  
  › Gap G1. A single missing flag cannot tell an unplugged drive from a web link nobody has fetched yet, so healthy remote items get badged as broken.
- `[ ]` **Three source kinds** — *unbuilt*  
  source_kind is local_file, remote_url or app_generated, with a single locator field holding either a path or a URL. Filesystem fields become nullable.  
  › Gap G2. Build 17 could already import a URL, but the model assumed every item was a file with a path, parent directory, inode and size.
- `[ ]` **Fuller metadata extraction** — *unbuilt*  
  Dimensions, duration, codecs and page counts, extracted at index time into sortable rows.  
  › Build 17 extracts four attribute keys. Seeking in the player needs duration; the inspector wants dimensions and codecs. ffmpeg is already a dependency.
- `[?]` **Move-detection ordering across volumes** — *decide*  
  Trying inode first is correct on a single disk and wrong across two, where inode numbers collide. Hashing first is always correct but costs a full read of every file.  
  › Needs a real library to answer — the right choice depends on how big yours is and whether it spans drives.
- `[?]` **Sort key storage and locale** — *decide*  
  Precomputing sort keys makes sorting a lookup, but sorting names correctly is language-dependent, so the keys become language-dependent too. Either store one set per language or accept a live comparison for the name sort only.  
  › Only affects the name sort. Every other sort is on a number or a date.

### Classification · 8

*Tags, facets, tiers, and how much of an item's description is filled in.*

- `[ ]` **Six facets in three tiers** — *rebuild*  
  Tier 1 Metadata: Format, Era — largely filled from file metadata. Tier 2 Classification: Environment, Action. Tier 3 Content: Attribute, Subject — pure human judgement. All facets optional per item.  
  › The three tiers are also the three layers the map cycles through: structural, then contextual, then interpretive.
- `[ ]` **Tag create, apply, remove — including in batch** — *rebuild*  
  Tags applied to one item or to a multi-selection at once.  
  › Batch operations everywhere are the main defence against classification fatigue, which is the named risk that kills the library at item forty.
- `[ ]` **Tag rename, delete, merge, reorder** — *rebuild*  
  Full tag management, separate from applying tags to items.  
  › Worked in Build 17's Tags view.
- `[ ]` **Near-duplicate tag detection** — *rebuild*  
  Catches tags that differ by a character or a plural, and offers a merge. Dismissing keeps both permanently.  
  › This is tier 1 of the suggestion ladder and needs no machine learning.
- `[ ]` **Metadata suggestions, accept-only** — *rebuild*  
  Format and Era proposed from file metadata, and a dominant-colour reading proposed as an Attribute. Nothing is ever applied automatically.  
  › Accept-only is the rule. Whether dominant colour is a useful suggestion at all is worth re-testing — it was flagged as cheap to keep and easy to drop.
- `[ ]` **Tag to Collector promotion** — *rebuild*  
  Turn a tag into a Collector, with a rename and an option to strip the original tag. Deliberately one-way.  
  › The rule the onboarding states once: tags describe, Collectors aggregate. Promotion being one-way is what keeps the two concepts from blurring.
- `[ ]` **Health as a score plus its components** — *unbuilt*  
  Keep the 0–3 score for ranking, and store facets_filled, title_quality, has_any_tag and unresolved_links alongside it.  
  › Gap G20. One integer cannot distinguish well-tagged-but-badly-named from well-named-but-untagged, and those need different prompts. In Build 17 the bucket labels are a switch statement inside the view.
- `[?]` **Sub-tags — keep or cut** — *decide*  
  The parent_tag_id column exists in Build 17 with no interface to use it. Six facets across three tiers may already give all the depth you want.  
  › One of three decisions the audit put to you and you have not yet answered.

### Structure and links · 12

*Collectors, boards, notes, and the edges between everything.*

- `[ ]` **Collectors as nestable folders** — *rebuild*  
  Create, rename, nest — and delete.  
  › Delete is called out separately because the audit found it had no path anywhere in Build 17's interface. It was a regression, not a design choice.
- `[ ]` **Collectors as boards** — *rebuild*  
  A Miro or Figma style freeform canvas holding media, notes and other nodes placed anywhere, with links drawn between them.  
  › The project-by-project workspace from the original brief.
- `[ ]` **Compass links — north, south, east, west** — *rebuild*  
  Four directional slots for relating any item to any other, with optional labels.  
  › The core relating gesture of the app.
- `[ ]` **Markdown notes stored as files on disk** — *rebuild*  
  Live preview, a read mode, and autosave. The note is a real .md file you could open in any other editor.  
  › Third-party-free and self-hosted was requirement one. Notes being plain files on your disk is what makes that true rather than claimed.
- `[ ]` **Board cards carry size and stacking order** — *unbuilt*  
  Cards get width, height and a z value, and text cards become real nodes rather than rows in a side table.  
  › Gap G10. Build 17 stores position only, so cards cannot be resized or brought to front, and a board's text cards are invisible to every other view.
- `[ ]` **North and south invert; east and west do not** — *unbuilt*  
  Viewed from the other end, a north link reads as south. A west link still reads as west, and an east link still reads as east.  
  › Gap G23, and the strongest argument for doing this work on paper. The first draft inverted all four. Marking something as opposing this item would have read as merely related from its own side — silently weakening a claim you made. No view before Compass reads a link from both ends at once, so nothing else would have caught it.
- `[ ]` **Edges store a kind; compass is derived from it** — *unbuilt*  
  tag_of, contains, compass_n, compass_s, compass_e, compass_w, wikilink, embed. A dropped tag becomes tag_of, a dropped collector becomes contains, anything else becomes a compass edge.  
  › Gap G7. Build 17 stores tagging and collector membership identically as direction='N', so what a row means is only recoverable by looking at what it points to.
- `[ ]` **Edges record who asserted them** — *unbuilt*  
  Each edge carries an origin and a status, so a fact you stated is distinguishable from a machine suggestion.  
  › Gap G8, and the precondition for ever shipping suggested links. The Inspector currently cannot tell the two apart.
- `[ ]` **Board-scoped links** — *unbuilt*  
  Links that exist only within one board, via a nullable scope_collector_id, surfaced elsewhere as a list in the item's inspect panel.  
  › Schema and backend support exist in Build 17 with no interface. Costs one nullable column to keep.
- `[ ]` **Wikilinks and embeds with a resolution index** — *unbuilt*  
  [[link]] and ![[embed]] are separate edge kinds. Each stores the raw text always and a resolved target when one is found. Creating a node with a matching title resolves waiting links; renaming re-resolves both directions.  
  › Gap G9. Build 17 resolves links by matching the title string at the moment of rendering, so renaming a note silently breaks every link to it and nothing can report how many links are broken.
- `[ ]` **A named write path** — *unbuilt*  
  link(source, target, compass), unlink(edge_id), reorder(edge_id, ordinal). Each bumps the item's revision once and emits one change event.  
  › Gap G24. Phase 0 defined nine ways to read the model and none to change it — invisible until Compass, which creates and destroys edges by dragging.
- `[ ]` **View preferences persisted per folder** — *unbuilt*  
  Layout, sort, grouping and density stored in the database against a scope, not held in component state.  
  › Gap G13. In Build 17 the layout lives in React state, so it is lost on remount and two panes showing the same collector can disagree. This is your data, so it belongs in a row.

### Views · 13

*The twelve surfaces. Each reads the model through exactly one projection.*

- `[ ]` **Library** — *rebuild*  
  Reads p_rows. The main list of everything.  
  › Blocked by twelve of the twenty gaps — the most of any view, which is the argument for converting it first.
- `[ ]` **Scattered** — *rebuild*  
  Reads p_rows and p_health. Untagged and under-described items, bucketed by how much is missing. A soft gate: items are usable but visibly unprocessed.  
  › The soft gate was a decision, not a default. Minimum ingest is zero required fields.
- `[ ]` **Miller columns** — *rebuild*  
  Reads p_tree. Finder-style cascading columns.  
  › Build 17 makes two backend calls per column and merges them in the view, because no projection returns a whole column.
- `[ ]` **Collector grid** — *rebuild*  
  Reads p_rows. The same rows as Library at a different density.  
  › Only differs from Library by density, and density had nowhere to persist until G13.
- `[ ]` **Board** — *rebuild*  
  Reads p_board, returning a viewport, cards sorted by stacking order, and edges.  
  › Nearly independent of the other views — its problems are almost all its own, so it can be worked in parallel.
- `[ ]` **Note editor** — *rebuild*  
  Reads p_note. Markdown editing with live preview.  
  › Cannot currently report which links are unresolved, because resolution happens at render time.
- `[ ]` **Inspector** — *rebuild*  
  Reads p_detail. Everything known about one item, including its compass slots.  
  › Also blocked by twelve gaps, which is the argument for converting it last — it touches more of the model than anything else.
- `[ ]` **Graph** — *rebuild*  
  Reads p_graph. The web of connecting clusters from the original brief, with node degree broken down by edge kind.  
  › Build 17 sizes nodes by tag usage and filters by whether a thumbnail file exists — reading storage details directly instead of asking the model.
- `[ ]` **Preview** — *rebuild*  
  Reads p_detail. Full-size viewing with playback, stepping through neighbours.  
  › Build 17 steps through siblings using a list of ids copied in as a prop, which goes stale the moment the underlying list changes.
- `[ ]` **Command palette** — *rebuild*  
  Reads p_search, which returns one ranked list marking each hit as matching on name, body or an associated tag.  
  › Build 17 makes two searches and interleaves them by hand, so ranking across the two is guesswork. Its type filters compare strings instead of asking the type system.
- `[ ]` **Discover** — *unbuilt*  
  Reads p_suggest. Pairs of items sharing two or more tags with no link between them, plus items with no connections at all.  
  › The entire no-machine-learning half of the original brief. The backend is written and tested in Build 17 and has never had an interface. Build it or delete it — the audit's first open decision.
- `[ ]` **Compass** — *unbuilt*  
  The four-slot relating surface, validated as a twelfth view after Phase 0 was written. Each slot entry carries the far node's full row so one call draws the whole compass.  
  › Validating it produced four further findings, including the east–west inversion error. Slot overflow is still open: four tiles fit a rail and forty do not, and the projection must return a total per direction whatever is decided.
- `[ ]` **Contour view** — *unbuilt*  
  A tag-boundary layout, drawing regions around groups rather than points and lines.  
  › The one view from the original concept that was never built at all.

### Content handling · 7

*Proxies, formats, playback and preview for image, video, audio, 3D and PDF.*

- `[ ]` **Image proxies with EXIF rotation** — *rebuild*  
  Versioned, so bumping the version rebuilds them all.  
  › The one proxy path that fully worked in Build 17.
- `[ ]` **Video proxies** — *rebuild*  
  Requires ffmpeg on the machine.  
  › Build 17 falls back to a generic glyph silently when ffmpeg is absent, so a missing dependency looks like a broken file.
- `[ ]` **Four proxy artefacts, tracked separately** — *unbuilt*  
  A grid thumbnail, a preview-sized render, a transcoded playable file and the original — each with its own reference, plus one shared version number and a state of not_applicable, pending, ready or failed.  
  › Gap G3. Build 17 has one thumbnail field for all four, which is why the graph's 'existing only' filter is literally a check for whether a proxy file is non-null.
- `[ ]` **Audio waveforms** — *unbuilt*  
  A rendered waveform standing in for a thumbnail.  
  › Never built. Audio shows a glyph only.
- `[ ]` **3D thumbnails** — *unbuilt*  
  A rendered still for OBJ, FBX, STL and glTF models.  
  › Never built — 3D was in the original brief and has only ever shown a glyph. FBX is the risky one: timebox it and ship OBJ first if it fights back.
- `[ ]` **PDF as the paginated type** — *unbuilt*  
  PDF resolves to preview, full_res, paginate, embed, tag, link, rename, delete and reveal — and not play, with no exception written anywhere.  
  › The test case for the capability registry. A multi-page TIFF would only need to declare the same conformance to inherit pagination for free.
- `[ ]` **Remote items actually fetched and cached** — *unbuilt*  
  A pasted URL becomes an item whose content is fetched, cached and previewed.  
  › Build 17 can add the item but never fetches or previews it. The content security policy already carries the permission for exactly this.

### Platform and stack · 13

*The frameworks, languages and libraries the app is built from.*

- `[x]` **SQLite via rusqlite, bundled** — *tested*  
  One file on disk is the single source of truth. Every window reads from it, and any change broadcasts so the others reload.  
  › That broadcast is what keeps a second window in step without the two sharing any memory.
- `[x]` **Capability registry as executable code** — *tested*  
  Twenty-one capabilities. A view asks can(item, 'play') and never asks what type something is. A capability is granted at the highest type where it is always true and inherited downward, then gated by a live check on availability, proxy readiness or item count.  
  › Gap G15. This means can() answers 'can, right now', which is the answer a button needs. It exists twice — capabilities.ts and capabilities.rs — and those two will drift the moment someone edits one.
- `[ ]` **Tauri v2 as the application shell** — *rebuild*  
  Chosen over Electron for small binaries and a fast native backend, and because it targets iOS and Android from the same codebase.  
  › Binary size matters specifically because of the export-package idea: the exported viewer is a slimmed build of the same app.
- `[ ]` **Rust backend** — *rebuild*  
  All indexing, hashing, proxy generation and database access. Exposed to the interface as named commands.  
  › Build 17's command layer grew to over eleven hundred lines in one file and became where bugs hid. Split it by domain from the start.
- `[ ]` **React and TypeScript, built with Vite** — *rebuild*  
  Strict mode on, with unused locals and parameters treated as errors.  
  › Standard Tauri frontend setup.
- `[ ]` **Zustand for interaction state only** — *rebuild*  
  Selection, active item and transient modes live in memory. Anything that should survive a restart is a database row.  
  › Drawing that line explicitly is what stops view preferences leaking into memory and disappearing.
- `[ ]` **Dockview for panes, native windows for pop-outs** — *rebuild*  
  Splittable, tabbed, pinnable panes. Popping a pane out opens a real second window rather than a browser pop-up.  
  › Dockview's own pop-out needs the app served over localhost, which widens the attack surface in a packaged app. A native window keeps the custom protocol, and the two stay in step through the database.
- `[ ]` **Supporting Rust crates** — *rebuild*  
  walkdir for traversal, blake3 for hashing, serde for the boundary, image for proxies, kamadak-exif for orientation, printpdf and qrcode for export. UUID v7 was written by hand rather than adding a crate.  
  › Two versions of the image crate are currently required because printpdf bundles an older one. That is a compile-time cost only, but it is the kind of thing that bites during an upgrade.
- `[ ]` **ffmpeg as an external dependency** — *rebuild*  
  Needed for video proxies and duration extraction.  
  › Not bundled. Its absence must be visible rather than silently degrading to a glyph.
- `[ ]` **Nine projections as the only read path** — *unbuilt*  
  p_rows, p_tree, p_board, p_graph, p_note, p_detail, p_health, p_suggest, p_search. A view reaches the data only through one of these, and gets rows already grouped, sorted and flattened.  
  › The rule behind the whole rebuild: if two views could disagree about something, it belongs in the model rather than in a view. Every recurring bug in Build 17 was two components disagreeing.
- `[ ]` **Frontend test coverage** — *unbuilt*  
  Tests for the classes of bug that kept recurring: portal mounting, drop targets, keyboard handling.  
  › Every interface bug in Build 17 was found by you rather than by a test. The one part with tests — the pane tree, twenty-three of them — never broke. That is not a coincidence.
- `[ ]` **Scale testing before the graph is called done** — *unbuilt*  
  A synthetic ten-thousand-item library, run against every view.  
  › The graph has never run above a few hundred nodes and pushes every node away from every other node on every frame, which gets quadratically slower as the library grows. The original plan called for this test before the graph was declared finished, and it never happened.
- `[ ]` **Build verification before any status report** — *unbuilt*  
  cargo test and npm run build both pass before anything is described as working.  
  › The single biggest process lesson from the rebuild chat. Use Claude Code for the build itself — it can compile and run the tests, which turns every round trip through your machine into an internal loop.

### Packaging and distribution · 3

*Getting the app onto a machine, and getting an archive out of it.*

- `[ ]` **Native installers on all three desktop platforms** — *rebuild*  
  .dmg and .app for macOS, .msi and .exe for Windows, .deb and AppImage for Linux, all from Tauri's own bundler.  
  › macOS builds must be compiled on a Mac. Set up a GitHub Actions workflow with a macOS runner early so distribution is never an afterthought.
- `[ ]` **Code signing and notarisation** — *unbuilt*  
  An unsigned .dmg triggers a Gatekeeper warning that the app is damaged or from an unidentified developer. Distribution-quality builds need an Apple Developer account, around ninety-nine dollars a year.  
  › For your own use, right-click then Open gets past it. For anyone else, it does not.
- `[ ]` **Export packages** — *unbuilt*  
  Bundle chosen content, its map, tags and notes into a standalone viewer that opens on a machine that has never seen Archiva — and that carries a way to get the full app.  
  › The headline idea from your very first message, still entirely unstarted. Technically it is a slimmed build of the same app with a database subset and proxies embedded, which is why binary size influenced the Tauri decision.

### Deferred capability · 4

*Named in the original brief, scheduled late on purpose.*

- `[ ]` **Remote access and mobile clients** — *unbuilt*  
  Tauri's iOS and Android targets from the same codebase, plus a local network server. Mobile acts as a viewer and editor for tags and notes; the desktop app keeps doing the indexing and proxy generation.  
  › Deliberately late. It multiplies the surface area of everything built before it.
- `[ ]` **Suggestion ladder beyond tier one** — *unbuilt*  
  Tier 1 — edit distance and tag co-occurrence — is already written and needs no machine learning. Anything above that waits.  
  › Only real use will tell you which suggestions you actually want, and the edge origin field from G8 has to exist first so a suggestion is never mistaken for something you asserted.
- `[?]` **Map level-cycling treatment** — *decide*  
  Animated two-dimensional transitions between the three tiers, or a genuinely layered three-dimensional scene.  
  › Parked until there is a working graph to judge it against. The three-tier structure gives cycling exactly three clean layers, which strengthens the case for the layered treatment.
- `[?]` **Name conflict check** — *decide*  
  Apache Archiva is a well-known open-source repository manager. Different field entirely, but worth a trademark and App Store search before any public release.  
  › Cheap now, expensive after a launch.

---
## The stack, in one place

| Layer | Choice | Settled? |
|---|---|---|
| Application shell | Tauri v2 | Yes — chosen over Electron for small binaries, a fast native backend, and iOS/Android from one codebase |
| Backend language | Rust | Yes |
| Database | SQLite, bundled via `rusqlite` | Yes — one file on disk is the single source of truth |
| Interface | React + TypeScript, built with Vite | Yes |
| Pane layout | Dockview, with native OS windows for pop-outs | Yes — Dockview's own pop-out needs the app served over localhost, which a packaged app should not do |
| In-memory state | Zustand — selection and transient modes only | Yes — anything that should survive a restart is a database row instead |
| Hashing | BLAKE3 | Yes |
| File traversal | `walkdir` | Yes |
| Data boundary | `serde` | Yes |
| Image proxies | `image`, plus `kamadak-exif` for orientation | Yes — currently pulls two versions of `image`, because `printpdf` bundles an older one |
| Export artefacts | `printpdf`, `qrcode` | Yes |
| Identifiers | UUID v7, written by hand | Yes — sixteen bytes did not justify a dependency |
| Video and duration | ffmpeg, external | Yes, but not bundled — its absence must be visible, not silent |

**Three requirements the stack was chosen against**, all from your original brief: fast file processing, fast indexing and rendering, and working on both mobile and desktop. Tauri v2 meets all three, with one architectural consequence baked in — mobile builds act as remote clients for viewing and editing tags and notes, while the desktop app does the indexing and proxy generation.

---

## The two rules the whole design rests on

Everything above follows from two sentences. If a build tool only reads two things from this document, make it these.

**If two views could disagree about something, it belongs in the model, not in a view.** Every recurring bug in Build 17 was two components disagreeing about the same fact. That is why there are nine projections and one capability registry rather than each view working it out for itself.

**Identity is minted, never derived.** A node's identifier comes from the indexer and never changes. It is not the file's contents, not its path, and not its name. Build 17 derived identity from content, so editing a note created a second copy of it.

---

## Sequencing

Not phases — the original six are obsolete, because the work was done out of order. This is what is left, ordered by what depends on what.

1. **Start the new project.** Fresh Tauri + React, no Build 17 code. Copy in `src-tauri/src/model/` and `migrations_model/001_model.sql` unchanged — those are the nine tested modules and they should not be rewritten.
2. **Shell and Library.** The first view on the new model, on UUID identifiers, with no integer ids anywhere. Library is blocked by twelve of the twenty gaps, which is exactly why it goes first — it proves the model.
3. **Run the backfill before the first scan.** Not after. Scanning first creates nodes at the same paths and the backfill will abort against the unique index on locator.
4. **The remaining views**, ending with the Inspector, which touches more of the model than anything else.
5. **Make it hold up.** Frontend tests for portal mounting, drop targets and keyboard handling. A synthetic ten-thousand-item library, and fix what breaks.
6. **Export packages.** The headline idea from your first message, still unstarted, and dependent on a stable build that can be slimmed.
7. **Remote and mobile**, then anything beyond tier one of the suggestion ladder.

**Use Claude Code for the build itself.** It can run `cargo test` and `npm run build`, which turns every compile error into an internal loop instead of a round trip through your machine. Ask it to verify rather than report — the instruction that matters is *run the tests before telling me anything works*.

---

## Open decisions

Five items are waiting on you. Three can be answered from the sofa; two need a real library in front of you.

| # | Decision | What it turns on |
|---|---|---|
| C8 | Sub-tags: keep or cut | Whether six facets across three tiers already give you the depth. The column exists with no interface. |
| V9 | Discover: build the interface or delete the backend | It is the entire no-machine-learning half of your original brief, written and tested, and invisible. |
| L3 | Map level-cycling: animated 2D, or a layered 3D scene | Parked until there is a working graph to judge against. |
| I12 | Move detection: try inode first, or hash first | Needs your real library. Inode is right on one disk and wrong across two; hashing is always right but reads every file. |
| I13 | Sort keys: store per language, or compare live | Only affects sorting by name. Everything else sorts on a number or a date. |

One more that is not a build decision: **L4, the name.** Apache Archiva is a well-known open-source repository manager in a completely different field. A trademark and App Store search is cheap now and expensive after a launch.

---

## Known risks, carried forward

These are not tasks. They are the things that have already gone wrong once, or that the plan has been warning about since version one.

- **Classification fatigue.** Six facets plus a compass can make adding an item feel like paperwork, and the library dies at item forty. The defences are built in: zero required fields on ingest, batch operations everywhere, and metadata pre-filling tier one. The real test is whether *you* keep tagging your own library. If you start skipping it, that is a design failure to fix before building anything else.
- **The graph has never run above a few hundred nodes.** It pushes every node away from every other node on every frame, which gets four times slower each time the library doubles. Test it at ten thousand before calling it finished.
- **The capability registry exists twice**, once in TypeScript and once in Rust. They will drift the moment someone edits one and not the other.
- **Two versions of the `image` crate.** A compile-time cost only, but the kind of thing that bites during an upgrade.
- **FBX is the risky 3D format.** Timebox it. Ship OBJ first if it fights back.
- **Silent degradation.** When ffmpeg is missing, Build 17 shows a generic icon, so a missing dependency is indistinguishable from a broken file. Make failures say what happened.

---

## Glossary

The document uses these terms because they are the words the code uses. Where a term carries an argument rather than just a label, the argument is spelled out.

**Node** — one row in the database. A photo, a note, a Collector and a tag are all nodes. They differ in what they hold and how they are drawn, not in their structure.

**Edge** — a stored connection between two nodes. Tagging, collector membership, compass links and wikilinks are all edges of different kinds.

**Model** — the shared description of what an item is and what can be true of it, held in one place. The alternative, which Build 17 did, is each view deciding for itself.

**View** — one surface in the app: Library, Board, Graph, Inspector and so on. Twelve of them.

**Projection** — a fixed, named way of reading the model for one view, which hands back exactly what that view needs, already sorted and grouped. There are nine. A view never queries the database directly. *The argument this word carries:* it makes it impossible for two views to compute the same thing differently, which is the bug class the rebuild exists to remove.

**Capability** — something an item can do right now: play, preview, paginate, tag, rename, delete. There are twenty-one. A button asks whether an item can play; it never asks what kind of file it is.

**Capability registry** — the single list of which types get which capabilities. *The argument:* a capability is granted at the highest type where it is always true and inherited downward, then checked against live conditions — is the drive plugged in, is the proxy ready. So `can()` means "can, right now", which is the answer a button actually needs.

**Conformance** — the statement that one type is a kind of another. A JPEG conforms to image, which conforms to data. This is what lets a filter for "images" pick up HEIC and Photoshop files without anyone maintaining a list.

**DAG** — a family tree where something can have more than one parent, and nothing loops back on itself. Types need this because a note is both a markdown file and an Archiva note, and both branches grant it different abilities.

**Schema** — the shape of the database: which tables exist and what columns they have.

**Migration** — a numbered script that changes the schema. `001_model.sql` is the first one for the new model.

**Backfill** — the one-off conversion that moves an existing Build 17 library into the new schema.

**Index (database)** — a lookup structure that makes finding rows fast. Unrelated to *indexing* a library.

**Unique index** — an index that also forbids duplicates. The unique index on file hash in Build 17 is the direct cause of its duplicate-node bug, which is why the new one is deliberately not unique.

**Transaction** — a group of changes that all happen or none do. The backfill is one transaction, so a failure part-way leaves nothing corrupted.

**Nullable** — a field allowed to be empty. Filesystem fields are nullable now because a web link has no file size.

**UUID** — a long random-looking identifier, unique without needing to check with anyone. **v7** is the variant that also sorts by when it was created, which gives sorting a free tiebreaker.

**Hash** — a short fingerprint computed from a file's contents. Same bytes, same fingerprint. **BLAKE3** is the particular method. *The argument:* it is now an attribute, not the identity — because a file's contents change when you edit it, and identity must not.

**Inode** — the number the operating system uses for a file, which stays the same when you rename it. First thing the reconciler checks. It is only unique per disk, which is what makes item I12 an open question.

**Reconciliation** — deciding what happened to a file between one scan and the next: new, edited, renamed, moved, copied, restored, deleted. Thirteen rules, read in order, first match wins.

**Idempotent** — running it twice changes nothing the second time. The test for the scanner: scan an unchanged library twice and nothing should be created, updated or logged.

**Frontmatter** — a small block at the top of a markdown file holding information about it. Notes carry their Archiva identifier there, which is how a note keeps its identity when you move it in Finder.

**Proxy** — a generated stand-in for a file: a thumbnail, a preview-sized render, or a version transcoded so it will play. *The argument:* those are four different things with four different readiness states, and Build 17 has one field for all of them.

**EXIF** — the metadata a camera writes into a photo, including which way up it was taken.

**UTI** — Apple's naming scheme for file types, like `public.jpeg`. Borrowed because the conformance relationships are already defined and correct.

**Materialised** — computed once and stored, rather than worked out each time it is needed. The list of types an item conforms to is materialised; its capabilities deliberately are not, because those depend on conditions that change by the second.

**Facet** — one of the six kinds of tag: Format, Era, Environment, Action, Attribute, Subject.

**Tier** — the three groups the facets fall into: Metadata, Classification, Content. Also the three layers the map cycles through.

**Compass** — the four directional slots — north, south, east, west — for relating one item to another. *The argument that nearly went wrong:* north and south invert when read from the other end; east and west do not. Getting that backwards would have quietly weakened claims you made.

**Wikilink** — a link written inside a note as `[[a title]]`. An **embed** uses `![[a title]]` and shows the thing rather than linking to it.

**Orphan** — an item with no connections at all. Discover finds these.

**Co-occurrence** — two items sharing tags. Discover proposes a link when two items share two or more tags and have none.

**z-order** — which card sits on top when cards overlap on a board.

**Viewport** — which part of a board you are currently looking at, and how far zoomed in.

**Crate** — a Rust library. **cargo** is Rust's build and test command. **npm** is the equivalent for JavaScript.

**Gatekeeper** — the macOS check that refuses to open apps from unidentified developers. **Notarisation** is Apple's process for getting past it, which needs a paid developer account.

**ffmpeg** — the standard external tool for reading and converting video and audio.
