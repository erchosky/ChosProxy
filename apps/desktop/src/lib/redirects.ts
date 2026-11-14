// Automatic redirect following in Repeater: which hops are allowed.

export type RedirectPolicy = "never" | "onSite" | "inScope" | "always";

/** A stored policy, with anything unknown read as "never". */
export function asRedirectPolicy(value: string | undefined): RedirectPolicy {
  return value === "onSite" || value === "inScope" || value === "always" ? value : "never";
}

export const REDIRECT_POLICIES: { id: RedirectPolicy; label: string }[] = [
  { id: "never", label: "Never" },
  { id: "onSite", label: "Same site only" },
  { id: "inScope", label: "In scope only" },
  { id: "always", label: "Always" },
];

/** Hops followed at most after one send. */
export const MAX_REDIRECTS = 10;

const REDIRECT_STATUSES = new Set([301, 302, 303, 307, 308]);

export function isRedirect(status: number | null): boolean {
  return status !== null && REDIRECT_STATUSES.has(status);
}

function origin(url: string): string | null {
  try {
    return new URL(url).origin;
  } catch {
    return null;
  }
}

/**
 * Whether to follow a redirect from `from` to `to` under `policy`
 * (`inScope` is whether `to` is in scope, only asked for that policy).
 */
export function shouldFollow(
  policy: RedirectPolicy,
  from: string,
  to: string,
  inScope: () => Promise<boolean>,
): Promise<boolean> {
  switch (policy) {
    case "never":
      return Promise.resolve(false);
    case "always":
      return Promise.resolve(true);
    case "onSite": {
      const a = origin(from);
      return Promise.resolve(a !== null && a === origin(to));
    }
    case "inScope":
      return inScope();
  }
}

/** The URL a Repeater request goes to (target + request-line path). */
export function requestUrl(target: string, request: string): string {
  const path = request.split(/\r?\n/, 1)[0]?.split(" ")[1] ?? "/";
  return /^https?:\/\//i.test(path) ? path : `${target}${path.startsWith("/") ? path : `/${path}`}`;
}
