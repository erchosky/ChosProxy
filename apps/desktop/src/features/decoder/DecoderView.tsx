import type { DecodeStep } from "../../bindings/DecodeStep";
import type { Operation } from "../../bindings/Operation";
import { Badge } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { base64ToBytes, hexRow } from "../../lib/bytes";
import { formatBytes } from "../../lib/format";
import { copyText } from "../../services/clipboard";
import { useDecoderStore } from "../../state/decoder";
import { OPERATION_GROUPS, operationLabel } from "./operations";

/** Rows of hex shown for binary output. */
const HEX_PREVIEW_ROWS = 32;

function OperationSelect({
  value,
  placeholder,
  onChange,
}: {
  value: Operation | "";
  placeholder?: string;
  onChange(op: Operation): void;
}) {
  return (
    <select
      aria-label={placeholder ?? "Operation"}
      value={value}
      onChange={(e) => {
        if (e.target.value) onChange(e.target.value as Operation);
      }}
    >
      {placeholder && <option value="">{placeholder}</option>}
      {OPERATION_GROUPS.map((g) => (
        <optgroup key={g.group} label={g.group}>
          {g.ops.map((o) => (
            <option key={o.op} value={o.op}>
              {o.label}
            </option>
          ))}
        </optgroup>
      ))}
    </select>
  );
}

function StepOutput({ step, index }: { step: DecodeStep; index: number }) {
  const load = useDecoderStore((s) => s.load);
  if (step.error) return <p className="field-error">{step.error}</p>;
  if (step.outputText !== null) {
    return (
      <div className="decoder-output">
        <textarea
          className="mono"
          readOnly
          aria-label={`Output of step ${index + 1}`}
          value={step.outputText}
          rows={Math.min(12, Math.max(2, step.outputText.split("\n").length))}
        />
        <div className="decoder-output-actions">
          <span className="muted">{formatBytes(step.size)}</span>
          <Button
            size="sm"
            variant="ghost"
            icon="copy"
            onClick={() => void copyText(step.outputText ?? "")}
          >
            Copy
          </Button>
          <Button size="sm" variant="ghost" onClick={() => load(step.outputText ?? "")}>
            Use as input
          </Button>
        </div>
      </div>
    );
  }
  const bytes = base64ToBytes(step.outputBase64);
  const rows = Math.min(HEX_PREVIEW_ROWS, Math.ceil(bytes.length / 16));
  return (
    <div className="decoder-output">
      <pre className="mono decoder-hex" aria-label={`Binary output of step ${index + 1}`}>
        {Array.from({ length: rows }, (_, r) => {
          const row = hexRow(bytes, r);
          return `${row.offset}  ${row.hex.padEnd(47)}  ${row.ascii}\n`;
        }).join("")}
      </pre>
      <div className="decoder-output-actions">
        <Badge>binary</Badge>
        <span className="muted">
          {formatBytes(step.size)}
          {rows * 16 < bytes.length ? ` (first ${rows * 16} bytes shown)` : ""}
        </span>
        <Button
          size="sm"
          variant="ghost"
          icon="copy"
          onClick={() => void copyText(step.outputBase64)}
        >
          Copy as Base64
        </Button>
      </div>
    </div>
  );
}

export function DecoderView() {
  const input = useDecoderStore((s) => s.input);
  const ops = useDecoderStore((s) => s.ops);
  const steps = useDecoderStore((s) => s.steps);
  const error = useDecoderStore((s) => s.error);
  const hint = useDecoderStore((s) => s.hint);
  const running = useDecoderStore((s) => s.running);
  const { setInput, setOps, smart } = useDecoderStore.getState();

  return (
    <div className="page decoder">
      <div className="page-header">
        <div>
          <h1>Decoder</h1>
          <p className="page-sub">
            Chain encodings, decompression, JWT inspection and hashes. Each step works on the output
            of the previous one. Everything runs locally.
          </p>
        </div>
      </div>
      <section className="card">
        <div className="card-head">
          <h2>Input</h2>
          <span className="spacer" />
          <span className="muted">{formatBytes(new TextEncoder().encode(input).length)}</span>
          <Button size="sm" variant="primary" onClick={() => void smart()} disabled={input === ""}>
            Smart decode
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setInput("")} disabled={input === ""}>
            Clear
          </Button>
        </div>
        <textarea
          className="mono decoder-input"
          aria-label="Decoder input"
          placeholder="Paste a value, or use “Decode” on a selection in any message viewer…"
          value={input}
          rows={6}
          onChange={(e) => setInput(e.target.value)}
        />
        {hint && <p className="muted">{hint}</p>}
        {error && <p className="field-error">{error}</p>}
      </section>
      {ops.map((op, i) => (
        // Steps have no identity besides their position.
        <section className="card decoder-step" key={i}>
          <div className="card-head">
            <strong>Step {i + 1}</strong>
            <OperationSelect
              value={op}
              onChange={(next) => setOps(ops.map((o, j) => (j === i ? next : o)))}
            />
            <span className="spacer" />
            {running && i === ops.length - 1 && <span className="muted">Running…</span>}
            <IconButton
              icon="trash"
              label={`Remove ${operationLabel(op)}`}
              onClick={() => setOps(ops.filter((_, j) => j !== i))}
            />
          </div>
          {steps[i] ? (
            <StepOutput step={steps[i]} index={i} />
          ) : (
            i > 0 && steps.length > 0 && <p className="muted">Not run: a previous step failed.</p>
          )}
        </section>
      ))}
      <div className="form-actions">
        <OperationSelect
          value=""
          placeholder="Add a step…"
          onChange={(op) => setOps([...ops, op])}
        />
        {ops.length > 0 && (
          <Button size="sm" variant="ghost" onClick={() => setOps([])}>
            Remove all steps
          </Button>
        )}
      </div>
    </div>
  );
}
