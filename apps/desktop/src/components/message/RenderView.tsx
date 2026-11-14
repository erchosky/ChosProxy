// Visual preview of an HTML response (ADR-0007): an iframe sandboxed without
// any `allow-*` token (no scripts, no same origin, no forms, no popups, no
// top navigation), fed through `srcdoc`, with a CSP that blocks every network
// load. The app's own CSP is inherited by the frame as well.

/** Policy injected ahead of the document; only inline styles and data: images. */
export const RENDER_CSP =
  "default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; " +
  "form-action 'none'; base-uri 'none'";

/** Largest document rendered. */
export const MAX_RENDER_CHARS = 2 * 1024 * 1024;

/** The `srcdoc` for an untrusted HTML body. */
export function renderDocument(html: string): string {
  return (
    `<!doctype html><meta http-equiv="Content-Security-Policy" content="${RENDER_CSP}">` +
    `<meta name="referrer" content="no-referrer">${html}`
  );
}

export function RenderView({ html, title }: { html: string; title: string }) {
  if (html.length > MAX_RENDER_CHARS) {
    return (
      <p className="callout callout-warn">
        The document is larger than 2 MB and is not rendered. Use the Pretty or Raw view.
      </p>
    );
  }
  return (
    <div className="render-view">
      <p className="render-note muted">
        Static preview: scripts, forms and network loads (remote images, styles, fonts) are blocked.
      </p>
      <iframe
        title={`${title} rendered`}
        sandbox=""
        referrerPolicy="no-referrer"
        srcDoc={renderDocument(html)}
      />
    </div>
  );
}
