// A row's identity in a list is **where it sits**, not what it is.
//
// One item can legitimately be in several places at once — that is what
// `contains` edges are for, and it is the whole point of Collectors. The
// Library also lists everything, so an item inside an expanded Collector
// appears twice on screen: once in the flat list and once under its folder.
// Both rows carry the same node id.
//
// Keying selection on the node id then breaks in exactly the two ways it was
// reported: both rows highlight at once, and the arrow keys find the *first*
// occurrence of the id and jump there. The fix is not a new kind of node — an
// "instance" would have to be kept in step with its original, which is work
// the single-node model already does for free, and it would give one item two
// identities, which is the bug class this rebuild exists to remove.
//
// Instead: a **placement key**. Selection, the cursor and type-ahead run on
// placements; everything that reads or writes the model runs on node ids. One
// item, many placements, and no confusion about which row you are on.

export type Placed = { id: string; depth: number };

/** The chain of ids from the top of the list down to this row, joined. Two
 * rows for one item under different parents get different keys; the same row
 * keeps its key across a refresh, which id-based selection also did and an
 * index-based key would not. */
export function placementKeys(rows: Placed[]): string[] {
  const keys: string[] = [];
  // The key of the last row seen at each depth, so a child can name its parent
  // without the caller having to hand us the tree it flattened.
  const parentAt: string[] = [];
  for (const row of rows) {
    const depth = Math.max(0, row.depth);
    const parent = depth > 0 ? (parentAt[depth - 1] ?? "") : "";
    const key = parent ? `${parent}>${row.id}` : row.id;
    keys.push(key);
    parentAt[depth] = key;
    // Anything deeper belongs to a row we have now left.
    parentAt.length = depth + 1;
  }
  return keys;
}

/** The node a placement refers to. */
export function nodeIdOf(key: string): string {
  const at = key.lastIndexOf(">");
  return at === -1 ? key : key.slice(at + 1);
}

/** Node ids for a set of placements, in order, each appearing once.
 *
 * What a batch write and the ↑/↓ fallback both want: the same photograph
 * selected in two places is still one photograph to tag, and stepping through
 * the list should not pause twice on it. */
export function nodeIdsOf(keys: Iterable<string>): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const key of keys) {
    const id = nodeIdOf(key);
    if (seen.has(id)) continue;
    seen.add(id);
    out.push(id);
  }
  return out;
}

/** Every placement of one node in this list. Used to keep a row highlighted
 * when the item was selected somewhere else — the item is the same, so both
 * placements deserve to look active, but only one of them is the cursor. */
export function placementsOf(keys: string[], nodeId: string): string[] {
  return keys.filter((k) => nodeIdOf(k) === nodeId);
}
