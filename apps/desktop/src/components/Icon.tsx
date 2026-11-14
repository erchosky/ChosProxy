// Small original icon set (24x24, stroke-based).
const PATHS = {
  dashboard: "M4 4h7v7H4zM13 4h7v4h-7zM13 10h7v10h-7zM4 13h7v7H4z",
  gauge: "M4 18a8 8 0 1 1 16 0M12 18l4-6M12 18h.01",
  agent:
    "M12 3v4M8 7h8a3 3 0 0 1 3 3v6a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3v-6a3 3 0 0 1 3-3zM9 12h.01M15 12h.01M10 16h4",
  history: "M4 6h16M4 12h16M4 18h10",
  nodes:
    "M6 7a2 2 0 1 0 0-4a2 2 0 0 0 0 4zM18 21a2 2 0 1 0 0-4a2 2 0 0 0 0 4zM6 7v5a4 4 0 0 0 4 4h4a4 4 0 0 1 4 1",
  beacon:
    "M12 21v-7M12 14a2 2 0 1 0 0-4a2 2 0 0 0 0 4zM8.5 7.5a5 5 0 0 1 7 0M5.5 4.5a9 9 0 0 1 13 0",
  listeners:
    "M12 12m-2 0a2 2 0 1 0 4 0a2 2 0 1 0 -4 0M7.8 7.8a6 6 0 0 0 0 8.4M16.2 16.2a6 6 0 0 0 0-8.4M5 5a10 10 0 0 0 0 14M19 19a10 10 0 0 0 0-14",
  settings:
    "M12 15a3 3 0 1 0 0-6a3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3a1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5a1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8a1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1a1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5a1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  search: "M11 18a7 7 0 1 0 0-14a7 7 0 0 0 0 14zM21 21l-5-5",
  play: "M7 5l12 7l-12 7z",
  stop: "M6 6h12v12H6z",
  pause: "M8 5v14M16 5v14",
  restart: "M4 12a8 8 0 1 0 2.4-5.7M4 4v4h4",
  plus: "M12 5v14M5 12h14",
  trash: "M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3",
  close: "M6 6l12 12M18 6L6 18",
  copy: "M9 9h11v11H9zM5 15H4V4h11v1",
  warning: "M12 3l10 18H2zM12 10v5M12 18v.01",
  folder: "M3 6h6l2 2h10v11H3z",
  file: "M6 3h8l4 4v14H6zM14 3v4h4",
  temp: "M12 7v5l3 2M12 21a9 9 0 1 0 0-18a9 9 0 0 0 0 18z",
  wrap: "M4 6h16M4 12h13a3 3 0 0 1 0 6h-4M4 18h6M13 16l-2 2l2 2",
  arrowUp: "M12 19V5M6 11l6-6l6 6",
  arrowDown: "M12 5v14M6 13l6 6l6-6",
  replace: "M4 7h14l-3-3M20 17H6l3 3",
  code: "M8 7l-5 5l5 5M16 7l5 5l-5 5M14 4l-4 16",
  api: "M8 4c-2 0-3 1-3 3v2c0 1-1 2-2 3c1 1 2 2 2 3v2c0 2 1 3 3 3M16 4c2 0 3 1 3 3v2c0 1 1 2 2 3c-1 1-2 2-2 3v2c0 2-1 3-3 3",
  back: "M15 6l-6 6l6 6",
  forward: "M9 6l6 6l-6 6",
  repeater: "M17 3l4 4l-4 4M3 11V9a2 2 0 0 1 2-2h16M7 21l-4-4l4-4M21 13v2a2 2 0 0 1-2 2H3",
  shield: "M12 3l8 3v6c0 5-3.5 8-8 9c-4.5-1-8-4-8-9V6z",
  log: "M5 4h14v16H5zM8 8h8M8 12h8M8 16h5",
  logger: "M4 5h16M4 9h16M4 13h10M4 17h7M17 15v6M14 18h6",
  decoder: "M8 7l-5 5l5 5M16 7l5 5l-5 5M13.5 5l-3 14",
  compare: "M8 4v16M16 4v16M4 8h4M16 16h4M4 16h4M16 8h4",
  bookmark: "M6 3h12v18l-6-4l-6 4z",
  websocket: "M4 8h11l-3-3M20 16H9l3 3",
  target: "M12 21a9 9 0 1 0 0-18a9 9 0 0 0 0 18zM12 16a4 4 0 1 0 0-8a4 4 0 0 0 0 8zM12 12h.01",
  sitemap: "M10 3h4v4h-4zM3 17h4v4H3zM10 17h4v4h-4zM17 17h4v4h-4zM12 7v5M5 17v-5h14v5M12 12v5",
  command: "M9 6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3z",
  check: "M5 12l5 5l9-10",
  cookie:
    "M12 3a9 9 0 1 0 9 9a3 3 0 0 1-3-3a3 3 0 0 1-3-3a3 3 0 0 1-3-3zM8 11h.01M11 16h.01M15 14h.01M8 15h.01",
  layers: "M12 3l9 5l-9 5l-9-5zM3 13l9 5l9-5",
  menu: "M5 12h.01M12 12h.01M19 12h.01",
  chevronDown: "M6 9l6 6l6-6",
  chevronRight: "M9 6l6 6l-6 6",
  bolt: "M13 2L4 14h7l-1 8l9-12h-7z",
  puzzle:
    "M9 4a2 2 0 1 1 4 0v1h4v4h1a2 2 0 1 1 0 4h-1v5h-5v-1a2 2 0 1 0-4 0v1H4v-5h1a2 2 0 1 0 0-4H4V5h5z",
  dice: "M5 4h14a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM8.5 8.5h.01M15.5 8.5h.01M12 12h.01M8.5 15.5h.01M15.5 15.5h.01",
  spider:
    "M12 8a3 3 0 1 0 0 6a3 3 0 0 0 0-6zM12 14v6M9 11H4l-2-3M15 11h5l2-3M9 13l-4 4l-1 4M15 13l4 4l1 4M10 9L7 5M14 9l3-4",
} as const;

export type IconName = keyof typeof PATHS;

interface IconProps {
  name: IconName;
  size?: number;
  className?: string;
}

export function Icon({ name, size = 16, className }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={className}
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
