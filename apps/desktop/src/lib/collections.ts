/** A collection in the Organizer's tree (levels are separated by "/"). */
export interface CollectionNode {
  /** Full name ("Auth/Login"). */
  path: string;
  /** Last level ("Login"). */
  label: string;
  /** 0 for a top-level collection. */
  depth: number;
  /** Items in it and in the collections under it. */
  count: number;
}

/** "a / b//c " → "a/b/c" (what the engine stores). */
export function normalizeCollection(name: string): string {
  return name
    .split("/")
    .map((l) => l.trim())
    .filter(Boolean)
    .join("/");
}

/**
 * The tree of collections for a list of item collections (one entry per
 * item; "" = none), as a flat list in display order. A parent that only
 * exists as part of a child's name is listed too.
 */
export function collectionTree(itemCollections: string[]): CollectionNode[] {
  const counts = new Map<string, number>();
  for (const raw of itemCollections) {
    const name = normalizeCollection(raw);
    if (!name) continue;
    const levels = name.split("/");
    for (let i = 1; i <= levels.length; i++) {
      const path = levels.slice(0, i).join("/");
      counts.set(path, (counts.get(path) ?? 0) + 1);
    }
  }
  return [...counts.keys()]
    .sort((a, b) => a.localeCompare(b))
    .map((path) => {
      const levels = path.split("/");
      return {
        path,
        label: levels[levels.length - 1] ?? path,
        depth: levels.length - 1,
        count: counts.get(path) ?? 0,
      };
    });
}

/** True when an item's collection is `selected` or under it ("" = all). */
export function inCollection(itemCollection: string, selected: string): boolean {
  if (selected === "") return true;
  return itemCollection === selected || itemCollection.startsWith(`${selected}/`);
}
