import type { Highlight } from "../bindings/Highlight";
import type { HistoryQuery } from "../bindings/HistoryQuery";
import type { HistorySort } from "../bindings/HistorySort";
import type { SiteFilter } from "../bindings/SiteFilter";
import type { Source } from "../bindings/Source";

/** Extensions hidden by the "Hide static assets" toggle. */
export const STATIC_EXTENSIONS = [
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "avif",
  "ico",
  "bmp",
  "svg",
  "woff",
  "woff2",
  "ttf",
  "otf",
  "eot",
  "css",
  "map",
  "mp4",
  "webm",
  "mp3",
];

export const STATUS_CLASSES = [
  { value: 2, label: "2xx" },
  { value: 3, label: "3xx" },
  { value: 4, label: "4xx" },
  { value: 5, label: "5xx" },
  { value: 0, label: "No response" },
] as const;

export const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] as const;

/** Tools that can produce traffic, in display order. */
export const SOURCES: readonly Source[] = [
  "proxy",
  "repeater",
  "fuzzer",
  "crawler",
  "scanner",
  "sequencer",
  "discovery",
  "plugin",
  "agent",
];

/** Row highlight colours, in display order. */
export const HIGHLIGHTS: readonly Highlight[] = [
  "red",
  "orange",
  "yellow",
  "green",
  "cyan",
  "blue",
  "purple",
  "gray",
];

export interface HistoryFilter {
  search: string;
  /** `search` is a regular expression. */
  regex: boolean;
  methods: string[];
  statusClasses: number[];
  hideStatic: boolean;
  inScopeOnly: boolean;
  annotatedOnly: boolean;
  sources: Source[];
  /** Restrict to one site map node. */
  site: SiteFilter | null;
  /** Only these MIME categories (`HTML`, `JSON`…). */
  mimes: string[];
  /** Sort column; `newestFirst` is its direction (descending). */
  sort: HistorySort;
  newestFirst: boolean;
}

/** MIME categories offered as filter chips. */
export const MIME_FILTERS = ["HTML", "JSON", "script", "XML", "CSS", "image", "text"] as const;

export const DEFAULT_FILTER: HistoryFilter = {
  search: "",
  regex: false,
  methods: [],
  statusClasses: [],
  hideStatic: false,
  inScopeOnly: false,
  annotatedOnly: false,
  sources: [],
  site: null,
  mimes: [],
  sort: "id",
  newestFirst: true,
};

/** The filter part of an engine query (shared by history, site map and search). */
export function toFilterQuery(filter: HistoryFilter): HistoryQuery {
  const search = filter.search.trim();
  return {
    offset: 0,
    limit: 1,
    order: filter.newestFirst ? "desc" : "asc",
    sort: filter.sort,
    search: search === "" ? null : search,
    searchRegex: filter.regex,
    inScopeOnly: filter.inScopeOnly,
    mimes: filter.mimes,
    sources: filter.sources,
    highlights: [],
    annotatedOnly: filter.annotatedOnly,
    site: filter.site,
    methods: filter.methods,
    statusClasses: filter.statusClasses,
    hideExtensions: filter.hideStatic ? STATIC_EXTENSIONS : [],
  };
}

/** Converts the UI filter into an engine query for one page. */
export function toQuery(filter: HistoryFilter, offset: number, limit: number): HistoryQuery {
  return { ...toFilterQuery(filter), offset, limit };
}

/** True if any narrowing filter is active. */
export function isFiltered(filter: HistoryFilter): boolean {
  return (
    filter.search.trim() !== "" ||
    filter.methods.length > 0 ||
    filter.statusClasses.length > 0 ||
    filter.hideStatic ||
    filter.inScopeOnly ||
    filter.annotatedOnly ||
    filter.sources.length > 0 ||
    filter.mimes.length > 0 ||
    filter.site !== null
  );
}

/** The narrowing filters cleared, keeping the sort order. */
export function clearedFilter(filter: HistoryFilter): HistoryFilter {
  return { ...DEFAULT_FILTER, sort: filter.sort, newestFirst: filter.newestFirst };
}

/** Human-readable label of a site filter, e.g. `https://lab.test:8443/api/`. */
export function siteLabel(site: SiteFilter): string {
  const defaultPort = site.scheme === "https" ? 443 : 80;
  const port = site.port === defaultPort ? "" : `:${site.port}`;
  return `${site.scheme}://${site.host}${port}${site.path}`;
}

/** Adds or removes a value from a list. */
export function toggle<T>(list: readonly T[], value: T): T[] {
  return list.includes(value) ? list.filter((v) => v !== value) : [...list, value];
}
