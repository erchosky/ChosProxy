import { useEffect, useState } from "react";
import type { CaptureConfig } from "../../bindings/CaptureConfig";
import type { ObservationLevel } from "../../bindings/ObservationLevel";
import type { TokenAnalysis } from "../../bindings/TokenAnalysis";
import type { TokenLocation } from "../../bindings/TokenLocation";
import { Badge } from "../../components/Badge";
import { type Bar, BarChart } from "../../components/BarChart";
import { Button, IconButton } from "../../components/Button";
import { SplitPane } from "../../components/SplitPane";
import { EmptyState, ErrorState, LoadingState } from "../../components/States";
import { parseTarget } from "../../lib/repeaterTarget";
import { useSequencerStore } from "../../state/sequencer";
import { useTasksStore } from "../../state/tasks";

/** Sequencer: collect tokens and look at how random they are. */
export function SequencerView() {
  const config = useSequencerStore((s) => s.config);
  const { init } = useSequencerStore.getState();
  useEffect(() => {
    void init();
  }, [init]);
  if (!config) return <LoadingState />;
  return (
    <div className="fuzzer-body">
      <SplitPane
        direction="horizontal"
        initial={0.4}
        first={<CaptureSetup config={config} />}
        second={<AnalysisPanel />}
      />
    </div>
  );
}

function locationOf(kind: TokenLocation["kind"], value: string): TokenLocation {
  return kind === "regex" ? { kind, pattern: value } : { kind, name: value };
}

function locationValue(l: TokenLocation): string {
  return l.kind === "regex" ? l.pattern : l.name;
}

function describe(l: TokenLocation): string {
  return l.kind === "cookie"
    ? `cookie ${l.name}`
    : l.kind === "header"
      ? `header ${l.name}`
      : `pattern ${l.pattern}`;
}

function CaptureSetup({ config }: { config: CaptureConfig }) {
  const targetText = useSequencerStore((s) => s.targetText);
  const candidates = useSequencerStore((s) => s.candidates);
  const pasted = useSequencerStore((s) => s.pasted);
  const starting = useSequencerStore((s) => s.starting);
  const { edit, setTargetText, start, setPasted, analyzePasted } = useSequencerStore.getState();
  const [error, setError] = useState<string | null>(null);
  const target = parseTarget(targetText);
  const location = config.location;
  const num = (
    key: "count" | "concurrency" | "requestsPerSecond" | "timeoutSecs",
    label: string,
    min: number,
    max: number,
  ) => (
    <label className="fuzzer-number">
      <span>{label}</span>
      <input
        type="number"
        min={min}
        max={max}
        value={config[key]}
        onChange={(e) => edit({ [key]: Number(e.target.value) })}
      />
    </label>
  );

  return (
    <section className="fuzzer-panel fuzzer-scroll" aria-label="Capture">
      <h3>Capture tokens</h3>
      <p className="muted fuzzer-help">
        Repeats a request that issues a token (a login or a page that sets a session cookie) and
        collects one token per response. Send a request here with “Send to Sequencer” on any
        request. Use it only on applications you are authorized to test.
      </p>
      <label className="repeater-target">
        <span>Target</span>
        <input
          className="mono"
          value={targetText}
          spellCheck={false}
          aria-invalid={"error" in target}
          onChange={(e) => setTargetText(e.target.value)}
        />
      </label>
      <textarea
        className="mono sequencer-request"
        rows={8}
        aria-label="Request"
        spellCheck={false}
        placeholder="GET /login HTTP/1.1&#10;Host: …"
        value={config.request}
        onChange={(e) => edit({ request: e.target.value })}
      />
      <h3>Where the token is</h3>
      {candidates.length > 0 && (
        <div
          className="sequencer-candidates"
          role="group"
          aria-label="Tokens found in the response"
        >
          {candidates.map((c, i) => (
            <button
              key={i}
              type="button"
              className="chip"
              aria-pressed={JSON.stringify(c.location) === JSON.stringify(location)}
              title={`Sample: ${c.sample}`}
              onClick={() => edit({ location: c.location })}
            >
              {describe(c.location)}
            </button>
          ))}
        </div>
      )}
      <div className="dialog-row">
        <select
          aria-label="Token location"
          value={location.kind}
          onChange={(e) =>
            edit({ location: locationOf(e.target.value as TokenLocation["kind"], "") })
          }
        >
          <option value="cookie">Cookie set by the response</option>
          <option value="header">Response header</option>
          <option value="regex">Regular expression (first group)</option>
        </select>
        <input
          className="mono sequencer-location"
          aria-label={location.kind === "regex" ? "Pattern" : "Name"}
          placeholder={location.kind === "regex" ? 'name="csrf" value="([^"]+)"' : "SESSIONID"}
          value={locationValue(location)}
          onChange={(e) => edit({ location: locationOf(location.kind, e.target.value) })}
        />
      </div>
      <div className="fuzzer-numbers">
        {num("count", "Tokens", 2, 20000)}
        {num("concurrency", "At a time", 1, 16)}
        {num("requestsPerSecond", "Per second (0 = no limit)", 0, 200)}
        {num("timeoutSecs", "Timeout (s)", 1, 300)}
      </div>
      {error && <div className="field-error">{error}</div>}
      <div className="fuzzer-start">
        <span className="muted">
          A few hundred tokens give a first picture; 1,000+ are needed for the bit analysis.
        </span>
        <span className="spacer" />
        <Button
          variant="primary"
          icon="play"
          disabled={starting || "error" in target}
          onClick={() => void start().then(setError)}
        >
          Start capture
        </Button>
      </div>

      <h3>Or analyze tokens you already have</h3>
      <textarea
        className="mono"
        rows={5}
        aria-label="Tokens, one per line"
        placeholder="One token per line"
        value={pasted}
        onChange={(e) => setPasted(e.target.value)}
      />
      <div className="dialog-row">
        <span className="muted">
          {pasted
            .split(/\r?\n/)
            .filter((l) => l.trim())
            .length.toLocaleString()}{" "}
          tokens
        </span>
        <span className="spacer" />
        <Button onClick={() => void analyzePasted()}>Analyze</Button>
      </div>
    </section>
  );
}

const LEVEL: Record<ObservationLevel, { tone: "ok" | "warn" | "danger"; label: string }> = {
  good: { tone: "ok", label: "✓ looks fine" },
  attention: { tone: "warn", label: "! look closer" },
  concern: { tone: "danger", label: "✕ concern" },
};

function AnalysisPanel() {
  const captures = useSequencerStore((s) => s.captures);
  const source = useSequencerStore((s) => s.source);
  const analysis = useSequencerStore((s) => s.analysis);
  const error = useSequencerStore((s) => s.analysisError);
  const analyzing = useSequencerStore((s) => s.analyzing);
  const sample = useSequencerStore((s) => s.sample);
  const { selectCapture, loadCaptures } = useSequencerStore.getState();
  const tasks = useTasksStore.getState();
  const capture = source?.kind === "capture" ? captures.find((c) => c.id === source.id) : undefined;
  const running = capture?.status === "running" || capture?.status === "paused";

  return (
    <section className="fuzzer-panel" aria-label="Analysis">
      <div className="fuzzer-bar">
        <select
          aria-label="Capture"
          value={source?.kind === "capture" ? source.id : ""}
          onChange={(e) => void selectCapture(Number(e.target.value))}
        >
          <option value="" disabled>
            {captures.length ? "Choose a capture…" : "No captures yet"}
          </option>
          {captures.map((c) => (
            <option key={c.id} value={c.id}>
              #{c.id} {c.name} — {c.progress || c.status}
            </option>
          ))}
        </select>
        {source?.kind === "pasted" && <Badge tone="accent">pasted tokens</Badge>}
        {capture && (
          <>
            <Badge tone={running ? "accent" : "neutral"}>{capture.status}</Badge>
            {capture.status === "running" && (
              <IconButton
                icon="pause"
                label="Pause"
                onClick={() => void tasks.pause(capture.id).then(loadCaptures)}
              />
            )}
            {capture.status === "paused" && (
              <IconButton
                icon="play"
                label="Resume"
                onClick={() => void tasks.resume(capture.id).then(loadCaptures)}
              />
            )}
            {running && (
              <IconButton
                icon="stop"
                label="Stop"
                onClick={() => void tasks.cancel(capture.id).then(loadCaptures)}
              />
            )}
            <Button size="sm" variant="ghost" onClick={() => void selectCapture(capture.id)}>
              {running ? "Analyze tokens so far" : "Analyze again"}
            </Button>
          </>
        )}
      </div>
      <div className="fuzzer-scroll sequencer-analysis">
        {analyzing && !analysis ? (
          <LoadingState />
        ) : error ? (
          <ErrorState title="No analysis" message={error} />
        ) : !analysis ? (
          <EmptyState title="Nothing analyzed yet">
            Start a capture, choose an earlier one, or paste tokens to see their statistics.
          </EmptyState>
        ) : (
          <Analysis analysis={analysis} sample={sample} />
        )}
      </div>
    </section>
  );
}

function Analysis({ analysis: a, sample }: { analysis: TokenAnalysis; sample: string[] }) {
  const limit = Math.min(a.bitsPerChar, a.sampleLimitBits);
  const positionBars: Bar[] = a.positions.map((p) => ({
    label: String(p.position + 1),
    value: p.entropyBits,
    detail: `${p.entropyBits.toFixed(2)} bits, ${p.distinct} different characters`,
    flagged: p.entropyBits < 0.5 * limit,
  }));
  const bitBars: Bar[] = a.bits.map((b) => ({
    label: String(b.bit + 1),
    value: b.onesRatio,
    detail: `set in ${(b.onesRatio * 100).toFixed(1)}% of tokens`,
    flagged: b.biased,
  }));
  const maxLen = Math.max(...a.lengths.map((l) => l.count), 1);
  const lengthBars: Bar[] = a.lengths.map((l) => ({
    label: String(l.length),
    value: l.count,
    detail: `${l.count.toLocaleString()} token(s)`,
  }));
  const posMax = Math.max(a.bitsPerChar, ...a.positions.map((p) => p.entropyBits), 0.1);
  const posTicks = [0, Math.round(posMax / 2), Math.round(posMax)].filter(
    (v, i, all) => all.indexOf(v) === i,
  );

  return (
    <div className="sequencer-results">
      <div className="sequencer-tiles">
        <Tile
          label="Estimated entropy"
          value={`${a.estimatedEntropyBits.toFixed(0)} bits`}
          hint={`of ${a.maxEntropyBits.toFixed(0)} the format allows`}
        />
        <Tile
          label="Tokens"
          value={a.samples.toLocaleString()}
          hint={`${a.unique.toLocaleString()} different`}
        />
        <Tile
          label="Length"
          value={a.minLength === a.maxLength ? `${a.minLength}` : `${a.minLength}–${a.maxLength}`}
          hint="characters"
        />
        <Tile
          label="Characters"
          value={a.encoding}
          hint={`${a.alphabet.length} seen · ${a.bitsPerChar.toFixed(2)} bits each`}
        />
      </div>

      <p className="muted sequencer-caveat">
        These measurements describe this sample. They can reveal weak tokens, but they cannot prove
        a generator is secure: read the observations together, not one number alone.
      </p>

      <ul className="sequencer-observations">
        {a.observations.map((o, i) => (
          <li key={i}>
            <Badge tone={LEVEL[o.level].tone}>{LEVEL[o.level].label}</Badge>
            <div>
              <strong>{o.title}</strong>
              <p className="muted">{o.detail}</p>
            </div>
          </li>
        ))}
      </ul>

      {positionBars.length > 0 && (
        <BarChart
          title={`Entropy per character position (tokens of length ${a.modalLength}; at most ${a.bitsPerChar.toFixed(1)} bits each)`}
          bars={positionBars}
          max={posMax}
          ticks={posTicks}
          formatTick={(v) => `${v}`}
          flaggedLabel="varies little"
          valueLabel="Entropy"
        />
      )}
      {bitBars.length > 0 && (
        <BarChart
          title="Share of tokens with each bit set (0.5 is ideal; shaded: expected range)"
          bars={bitBars}
          max={1}
          ticks={[0, 0.5, 1]}
          band={{
            low: 0.5 - a.bitBand,
            high: 0.5 + a.bitBand,
            label: `99% of random bits fall within 0.5 ± ${a.bitBand.toFixed(3)}`,
          }}
          flaggedLabel="outside the expected range"
          valueLabel="Set in"
        />
      )}
      {lengthBars.length > 1 && (
        <BarChart
          title="Token lengths"
          bars={lengthBars}
          max={maxLen}
          ticks={[0, maxLen]}
          formatTick={(v) => v.toLocaleString()}
          valueLabel="Tokens"
          height={120}
        />
      )}

      <details className="sequencer-sample">
        <summary>First tokens of the sample</summary>
        <ol className="mono selectable">
          {sample.map((t, i) => (
            <li key={i}>{t}</li>
          ))}
        </ol>
      </details>
    </div>
  );
}

function Tile({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div className="sequencer-tile">
      <span className="muted">{label}</span>
      <strong>{value}</strong>
      <span className="muted">{hint}</span>
    </div>
  );
}
