import { clearedFilter, siteLabel } from "../../lib/filters";
import type { SiteNode } from "../../lib/siteTree";
import { copyText } from "../../services/clipboard";
import { useAppStore } from "../../state/app";
import { useEngageStore } from "../../state/engage";
import { type MenuEntry, SEPARATOR } from "../../state/contextMenu";
import { useLoggerStore } from "../../state/history";
import { useScopeStore } from "../../state/scope";
import { useTasksStore } from "../../state/tasks";
import { crawlStart, exchangeMenu } from "../../state/tools";

export function siteOf(node: SiteNode) {
  return { scheme: node.scheme, host: node.host, port: node.port, path: node.path };
}

/** Lists the node's requests in the Logger (it shows every tool's traffic). */
export function showInLogger(node: SiteNode) {
  const logger = useLoggerStore.getState();
  logger.setFilter({ ...clearedFilter(logger.filter), site: siteOf(node) });
  useAppStore.getState().setView("logger");
}

/** Right-click / Shift+F10 menu of a site map node. */
export function nodeMenu(node: SiteNode): MenuEntry[] {
  const url = siteLabel(siteOf(node));
  const scope = useScopeStore.getState();
  const nodeEntries: MenuEntry[] = [
    {
      label: `Show ${node.count} request${node.count === 1 ? "" : "s"}`,
      icon: "history",
      onSelect: () => showInLogger(node),
    },
  ];
  if (node.entry) {
    return [
      ...nodeEntries,
      SEPARATOR,
      ...exchangeMenu({ id: node.entry.lastId, url, inScope: node.entry.inScope }),
    ];
  }
  return [
    { heading: url },
    ...nodeEntries,
    {
      label: "Crawl from here…",
      icon: "spider",
      onSelect: () => void useTasksStore.getState().openCrawl([crawlStart(url)]),
    },
    {
      label: "Analyze target…",
      icon: "target",
      onSelect: () => useEngageStore.getState().openAnalyzer(siteOf(node)),
    },
    {
      label: "Discover content here…",
      icon: "search",
      onSelect: () => void useTasksStore.getState().openDiscovery(url),
    },
    SEPARATOR,
    {
      label: node.kind === "origin" ? "Add host to scope" : "Add folder to scope",
      icon: "target",
      onSelect: () => void scope.addUrl(url, false, node.kind !== "origin"),
    },
    {
      label: node.kind === "origin" ? "Exclude host from scope" : "Exclude folder from scope",
      icon: "target",
      onSelect: () => void scope.addUrl(url, true, node.kind !== "origin"),
    },
    {
      label: "Copy URL",
      icon: "copy",
      onSelect: () =>
        void copyText(url).then((ok) =>
          useAppStore
            .getState()
            .notify(ok ? "info" : "error", ok ? "URL copied" : "Could not copy to the clipboard"),
        ),
    },
  ];
}
