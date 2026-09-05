// Which folders are open in the hierarchy, and the rule that only one branch
// is open at a time.
//
// A tree where any number of folders can be open at once is how the Library
// became unreadable: several folders' contents interleaved down the list, and
// no way to tell at a glance which rows belonged to which. One open branch —
// a **spine** from the root down to whatever you last opened — keeps the list
// short and keeps every visible row's parentage obvious.
//
// The spine is read straight off a placement key, which already carries the
// chain of ids from the root down to the row (see lib/placement.ts). So
// opening a folder is not a search through the tree; it is the key it was
// clicked on.

import { nodeIdOf } from "./placement";

/** Every id along a placement key, root first. */
export function spineOf(key: string): string[] {
  return key.split(">").filter(Boolean);
}

/**
 * Open the folder at `key`, closing every branch that is not on the way to
 * it; or close it if it is already open.
 *
 * Closing a folder closes what is inside it too — its descendants are on the
 * spine below it, and leaving them in the open set would make reopening the
 * parent spring the whole branch back.
 */
export function toggle(expanded: string[], key: string): string[] {
  const spine = spineOf(key);
  const id = nodeIdOf(key);
  if (expanded.includes(id)) {
    // Keep only what is above it.
    return spine.slice(0, spine.indexOf(id));
  }
  return spine;
}

/** Open the folder at `key` without closing it if it is already open — what
 * → does, where the gesture means "in", not "toggle". */
export function open(expanded: string[], key: string): string[] {
  const spine = spineOf(key);
  return expanded.includes(nodeIdOf(key)) ? expanded : spine;
}

/** Close the folder at `key`, and anything inside it.
 *
 * Derived from the key rather than from the current open set: with one branch
 * open at a time, what is above this folder *is* the rest of the open set. */
export function close(key: string): string[] {
  const spine = spineOf(key);
  return spine.slice(0, spine.indexOf(nodeIdOf(key)));
}

export function isOpen(expanded: string[], id: string): boolean {
  return expanded.includes(id);
}
