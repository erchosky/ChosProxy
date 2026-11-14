import { useMemo, useState } from "react";
import { Button, IconButton } from "../../components/Button";
import { EmptyState } from "../../components/States";
import { Segmented, type TabItem } from "../../components/Tabs";
import {
  type DiffMode,
  type DiffResult,
  type FieldChange,
  diffHttp,
  diffText,
} from "../../lib/diff";
import { formatBytes } from "../../lib/format";
import { type CompareItem, useComparerStore } from "../../state/comparer";

type Mode = "http" | DiffMode;

const MODES: readonly TabItem<Mode>[] = [
  { id: "http", label: "HTTP", title: "Status, headers, parameters, cookies and body" },
  { id: "line", label: "Lines" },
  { id: "word", label: "Words" },
  { id: "byte", label: "Bytes", title: "UTF-8 bytes in hex" },
];
/** Tokens rendered at most; the rest is summarized. */
const MAX_RENDERED = 30_000;

export function ComparerView() {
  const items = useComparerStore((s) => s.items);
  const aId = useComparerStore((s) => s.a);
  const bId = useComparerStore((s) => s.b);
  const [mode, setMode] = useState<Mode>("http");
  const a = items.find((i) => i.id === aId) ?? null;
  const b = items.find((i) => i.id === bId) ?? null;

  return (
    <div className="comparer">
      <ItemList />
      <div className="comparer-main">
        <div className="filters">
          <Segmented<Mode> items={MODES} value={mode} onChange={setMode} label="Comparison mode" />
          <span className="spacer" />
          <span className="mono muted comparer-slots">
            A: {a?.label ?? "—"} · B: {b?.label ?? "—"}
          </span>
          <Button size="sm" variant="ghost" onClick={() => useComparerStore.getState().swap()}>
            Swap A/B
          </Button>
        </div>
        {!a || !b ? (
          <EmptyState title="Choose two items">
            Send requests or responses here from History, the Logger or Search (“Compare”), or paste
            text on the left. Pick one as A and one as B.
          </EmptyState>
        ) : mode === "http" ? (
          <HttpComparison a={a} b={b} />
        ) : (
          <TextComparison a={a.text} b={b.text} mode={mode} />
        )}
      </div>
    </div>
  );
}

function ItemList() {
  const items = useComparerStore((s) => s.items);
  const a = useComparerStore((s) => s.a);
  const b = useComparerStore((s) => s.b);
  const { add, remove, setSlot } = useComparerStore.getState();
  const [text, setText] = useState("");
  return (
    <aside className="comparer-items" aria-label="Items to compare">
      <form
        className="comparer-paste"
        onSubmit={(e) => {
          e.preventDefault();
          add(`Text ${new Date().toLocaleTimeString()}`, text);
          setText("");
        }}
      >
        <textarea
          className="mono"
          aria-label="Text to compare"
          placeholder="Paste text to compare…"
          rows={4}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <Button type="submit" size="sm" disabled={text === ""}>
          Add text
        </Button>
      </form>
      {items.length === 0 ? (
        <p className="muted">No items yet.</p>
      ) : (
        <ul>
          {items.map((i) => (
            <li key={i.id} className="comparer-item">
              <div className="chip-group" role="group" aria-label={`Use ${i.label} as`}>
                <button
                  type="button"
                  className="chip"
                  aria-pressed={a === i.id}
                  onClick={() => setSlot("a", i.id)}
                >
                  A
                </button>
                <button
                  type="button"
                  className="chip"
                  aria-pressed={b === i.id}
                  onClick={() => setSlot("b", i.id)}
                >
                  B
                </button>
              </div>
              <span className="comparer-item-label" title={i.label}>
                {i.label}
                <span className="muted">
                  {" "}
                  · {formatBytes(new TextEncoder().encode(i.text).length)}
                </span>
              </span>
              <IconButton icon="trash" label={`Remove ${i.label}`} onClick={() => remove(i.id)} />
            </li>
          ))}
        </ul>
      )}
    </aside>
  );
}

function DiffBody({ result, mode }: { result: DiffResult; mode: DiffMode }) {
  let budget = MAX_RENDERED;
  const sep = mode === "byte" ? " " : "";
  const parts = [];
  for (const [i, op] of result.ops.entries()) {
    if (budget <= 0) break;
    const tokens = op.tokens.slice(0, budget);
    budget -= tokens.length;
    const text = tokens.join(sep) + (mode === "byte" ? " " : "");
    parts.push(
      op.kind === "equal" ? (
        <span key={i}>{text}</span>
      ) : op.kind === "insert" ? (
        <ins key={i}>{text}</ins>
      ) : (
        <del key={i}>{text}</del>
      ),
    );
  }
  return (
    <>
      <pre className={`mono diff diff-${mode}`}>{parts}</pre>
      {budget <= 0 && (
        <p className="muted">Only the first {MAX_RENDERED.toLocaleString()} tokens are shown.</p>
      )}
    </>
  );
}

function DiffSummary({ result, unit }: { result: DiffResult; unit: string }) {
  return (
    <p className="diff-summary">
      <span className="diff-added">+{result.added}</span>{" "}
      <span className="diff-removed">−{result.removed}</span> {unit}
      {result.coarse && " · too large or too different for a detailed comparison"}
    </p>
  );
}

function TextComparison({ a, b, mode }: { a: string; b: string; mode: DiffMode }) {
  const result = useMemo(() => diffText(a, b, mode), [a, b, mode]);
  const unit = mode === "line" ? "lines" : mode === "word" ? "words" : "bytes";
  return (
    <div className="comparer-result">
      <DiffSummary result={result} unit={unit} />
      {a === b ? (
        <p className="muted">The two items are identical.</p>
      ) : (
        <DiffBody result={result} mode={mode} />
      )}
    </div>
  );
}

function ChangeTable({ title, changes }: { title: string; changes: FieldChange[] }) {
  if (changes.length === 0) return null;
  return (
    <section className="change-table">
      <h3>
        {title} <span className="muted">{changes.length}</span>
      </h3>
      <table>
        <thead>
          <tr>
            <th>Name</th>
            <th>A</th>
            <th>B</th>
          </tr>
        </thead>
        <tbody>
          {changes.map((c, i) => (
            <tr
              key={`${c.name}-${i}`}
              className={c.a === null ? "added" : c.b === null ? "removed" : "changed"}
            >
              <td className="mono">{c.name}</td>
              <td className="mono">{c.a ?? <span className="muted">absent</span>}</td>
              <td className="mono">{c.b ?? <span className="muted">absent</span>}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

function HttpComparison({ a, b }: { a: CompareItem; b: CompareItem }) {
  const d = useMemo(() => diffHttp(a.text, b.text), [a.text, b.text]);
  if (d.identical) {
    return (
      <div className="comparer-result">
        <p className="muted">The two messages are identical.</p>
      </div>
    );
  }
  return (
    <div className="comparer-result">
      <p className="diff-summary">
        Size {formatBytes(d.sizes.a)} → {formatBytes(d.sizes.b)} · body {formatBytes(d.sizes.bodyA)}{" "}
        → {formatBytes(d.sizes.bodyB)}
      </p>
      {d.firstLine && <ChangeTable title="Request / status line" changes={[d.firstLine]} />}
      <ChangeTable title="Headers" changes={d.headers} />
      <ChangeTable title="Query parameters" changes={d.query} />
      <ChangeTable title="Cookies" changes={d.cookies} />
      <ChangeTable title="Body parameters" changes={d.form} />
      <h3>Body</h3>
      {d.body.added + d.body.removed === 0 ? (
        <p className="muted">Bodies are identical.</p>
      ) : (
        <>
          <DiffSummary result={d.body} unit="lines" />
          <DiffBody result={d.body} mode="line" />
        </>
      )}
    </div>
  );
}
