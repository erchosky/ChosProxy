import type { Inline } from "../lib/markdown";
import { parseMarkdown } from "../lib/markdown";

function InlineNodes({ inline }: { inline: Inline[] }) {
  return (
    <>
      {inline.map((n, i) => {
        switch (n.kind) {
          case "code":
            return <code key={i}>{n.text}</code>;
          case "strong":
            return <strong key={i}>{n.text}</strong>;
          case "em":
            return <em key={i}>{n.text}</em>;
          case "link":
            // Shown, not followed: notes may contain attacker-controlled URLs.
            return (
              <span key={i} className="md-link" title={n.url}>
                {n.text} <span className="muted">({n.url})</span>
              </span>
            );
          default:
            return <span key={i}>{n.text}</span>;
        }
      })}
    </>
  );
}

/** Renders a Markdown subset as React elements (never as HTML). */
export function Markdown({ source }: { source: string }) {
  const blocks = parseMarkdown(source);
  return (
    <div className="markdown selectable">
      {blocks.map((b, i) => {
        switch (b.kind) {
          case "heading": {
            const Tag = `h${Math.min(b.level + 1, 6)}` as "h2";
            return (
              <Tag key={i}>
                <InlineNodes inline={b.inline} />
              </Tag>
            );
          }
          case "paragraph":
            return (
              <p key={i}>
                <InlineNodes inline={b.inline} />
              </p>
            );
          case "quote":
            return (
              <blockquote key={i}>
                <InlineNodes inline={b.inline} />
              </blockquote>
            );
          case "code":
            return (
              <pre key={i} className="mono">
                {b.text}
              </pre>
            );
          case "rule":
            return <hr key={i} />;
          case "list": {
            const List = b.ordered ? "ol" : "ul";
            return (
              <List key={i}>
                {b.items.map((item, j) => (
                  <li key={j}>
                    <InlineNodes inline={item} />
                  </li>
                ))}
              </List>
            );
          }
        }
      })}
    </div>
  );
}
