import type { SiteMapEntry } from "../bindings/SiteMapEntry";
import type { SiteMark } from "../bindings/SiteMark";
import type { SiteMarkEntry } from "../bindings/SiteMarkEntry";
import { siteLabel } from "./filters";

/** A node of the site map tree: an origin, a folder or an endpoint. */
export interface SiteNode {
  /** Unique key: origin label + path. */
  key: string;
  kind: "origin" | "folder" | "endpoint";
  /** Text shown in the tree. */
  label: string;
  scheme: string;
  host: string;
  port: number;
  /** `/` for an origin, `/a/` for a folder, `/a/b` for an endpoint. */
  path: string;
  /** The endpoint seen at exactly this path, if any (folders can have one, e.g. `/a/`). */
  entry: SiteMapEntry | null;
  children: SiteNode[];
  /** Exchanges in this subtree. */
  count: number;
  /** Endpoints in this subtree. */
  endpoints: number;
  /** Some endpoint in this subtree is in scope. */
  inScope: boolean;
  mark: SiteMark | null;
}

function originKey(e: { scheme: string; host: string; port: number }): string {
  return siteLabel({ ...e, path: "" });
}

function newNode(
  kind: SiteNode["kind"],
  label: string,
  origin: { scheme: string; host: string; port: number },
  path: string,
): SiteNode {
  return {
    key: `${originKey(origin)}${path}`,
    kind,
    label,
    scheme: origin.scheme,
    host: origin.host,
    port: origin.port,
    path,
    entry: null,
    children: [],
    count: 0,
    endpoints: 0,
    inScope: false,
    mark: null,
  };
}

function sortTree(node: SiteNode) {
  // Folders first, then endpoints, each alphabetically.
  node.children.sort((a, b) => {
    const fa = a.kind === "endpoint" ? 1 : 0;
    const fb = b.kind === "endpoint" ? 1 : 0;
    return fa - fb || a.label.localeCompare(b.label);
  });
  for (const c of node.children) sortTree(c);
}

/** Builds the Host → folders → endpoints tree from flat site map entries. */
export function buildSiteTree(entries: SiteMapEntry[], marks: SiteMarkEntry[] = []): SiteNode[] {
  const origins = new Map<string, SiteNode>();
  const index = new Map<string, SiteNode>();
  const roots: SiteNode[] = [];

  for (const e of entries) {
    const key = originKey(e);
    let origin = origins.get(key);
    if (!origin) {
      origin = newNode("origin", key, e, "/");
      origins.set(key, origin);
      index.set(origin.key, origin);
      roots.push(origin);
    }
    const segments = e.path.split("/").slice(1);
    // A trailing "/" yields an empty last segment: the entry belongs to the folder.
    const isFolderEntry = segments[segments.length - 1] === "";
    const folders = isFolderEntry ? segments.slice(0, -1) : segments.slice(0, -1);
    const chain: SiteNode[] = [origin];
    let parent = origin;
    let path = "/";
    for (const seg of folders) {
      path += `${seg}/`;
      const k = `${key}${path}`;
      let folder = index.get(k);
      if (!folder) {
        folder = newNode("folder", `${seg}/`, e, path);
        index.set(k, folder);
        parent.children.push(folder);
      }
      chain.push(folder);
      parent = folder;
    }
    let target = parent;
    if (!isFolderEntry) {
      const name = segments[segments.length - 1] ?? "";
      const k = `${key}${e.path}`;
      let leaf = index.get(k);
      if (!leaf) {
        leaf = newNode("endpoint", name, e, e.path);
        index.set(k, leaf);
        parent.children.push(leaf);
      }
      chain.push(leaf);
      target = leaf;
    }
    target.entry = e;
    for (const n of chain) {
      n.count += e.count;
      n.endpoints += 1;
      n.inScope ||= e.inScope;
    }
  }
  for (const m of marks) {
    const node = index.get(`${originKey(m)}${m.path}`);
    if (node) node.mark = m.mark;
  }
  roots.sort((a, b) => a.host.localeCompare(b.host) || a.port - b.port);
  for (const r of roots) sortTree(r);
  return roots;
}

/** Every endpoint entry in a subtree, depth first. */
export function entriesUnder(node: SiteNode): SiteMapEntry[] {
  const out: SiteMapEntry[] = [];
  const walk = (n: SiteNode) => {
    if (n.entry) out.push(n.entry);
    for (const c of n.children) walk(c);
  };
  walk(node);
  return out;
}

/** Finds a node by key. */
export function findNode(roots: SiteNode[], key: string): SiteNode | null {
  for (const r of roots) {
    if (r.key === key) return r;
    if (key.startsWith(r.key.slice(0, -1))) {
      const found = findNode(r.children, key);
      if (found) return found;
    }
  }
  return null;
}
