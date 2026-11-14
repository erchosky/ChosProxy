import { useEffect, useState } from "react";
import type { PoolConfig } from "../../bindings/PoolConfig";
import type { PoolsConfig } from "../../bindings/PoolsConfig";
import type { PoolsView } from "../../bindings/PoolsView";
import { Button, IconButton } from "../../components/Button";
import { LoadingState } from "../../components/States";
import { Switch } from "../../components/Switch";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";

/** A new pool with a name not taken yet. */
export function newPool(taken: string[]): PoolConfig {
  let n = taken.length + 1;
  const used = new Set(taken.map((t) => t.trim().toLowerCase()));
  while (used.has(`pool ${n}`)) n += 1;
  return {
    name: `Pool ${n}`,
    concurrency: 10,
    requestsPerSecond: 0,
    randomDelayMs: 0,
    backOff: true,
  };
}

/** Renames a pool and moves its tool assignments along. */
export function renamePool(config: PoolsConfig, index: number, name: string): PoolsConfig {
  const old = config.pools[index]?.name.trim().toLowerCase();
  const assignments: Record<string, string> = {};
  for (const [tool, pool] of Object.entries(config.assignments)) {
    assignments[tool] = pool.trim().toLowerCase() === old ? name : pool;
  }
  return {
    pools: config.pools.map((p, i) => (i === index ? { ...p, name } : p)),
    assignments,
  };
}

/** Removes a pool; the tools assigned to it go back to their own limits. */
export function removePool(config: PoolsConfig, index: number): PoolsConfig {
  const old = config.pools[index]?.name.trim().toLowerCase();
  return {
    pools: config.pools.filter((_, i) => i !== index),
    assignments: Object.fromEntries(
      Object.entries(config.assignments).filter(([, p]) => p.trim().toLowerCase() !== old),
    ),
  };
}

function toConfig(view: PoolsView): PoolsConfig {
  return { pools: view.pools.map((p) => p.config), assignments: { ...view.assignments } };
}

/** Resource pools (M26): limits shared by every tool assigned to a pool. */
export function ResourcePoolsDialog({ onClose }: { onClose(): void }) {
  const [view, setView] = useState<PoolsView | null>(null);
  const [draft, setDraft] = useState<PoolsConfig | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    const load = () =>
      api
        .resourcePools()
        .then((v) => {
          if (!live) return;
          setView(v);
          setDraft((d) => d ?? toConfig(v));
        })
        .catch((e) => live && setError(toIpcError(e).message));
    void load();
    const t = window.setInterval(() => void load(), 1000);
    return () => {
      live = false;
      window.clearInterval(t);
    };
  }, []);

  async function save() {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      const v = await api.saveResourcePools(draft);
      setView(v);
      setDraft(toConfig(v));
      useAppStore.getState().notify("info", "Resource pools saved");
    } catch (e) {
      setError(toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  const update = (i: number, patch: Partial<PoolConfig>) =>
    setDraft(
      (d) => d && { ...d, pools: d.pools.map((p, j) => (j === i ? { ...p, ...patch } : p)) },
    );
  const live = (name: string) =>
    view?.pools.find((p) => p.config.name.trim().toLowerCase() === name.trim().toLowerCase());

  return (
    <div className="palette-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog dialog-wide"
        role="dialog"
        aria-modal="true"
        aria-label="Resource pools"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <header className="dialog-head">
          <strong>Resource pools</strong>
        </header>
        <p className="muted">
          A pool limits everything assigned to it together: two crawls, or a crawl and a Fuzzer
          attack, share its requests in flight and its rate. Each task's own limits still apply. A
          tool without a pool uses only its own limits.
        </p>
        {!draft || !view ? (
          error ? (
            <p className="callout callout-danger">{error}</p>
          ) : (
            <LoadingState />
          )
        ) : (
          <>
            <table className="endpoint-table pools-table" aria-label="Pools">
              <thead>
                <tr>
                  <th>Name</th>
                  <th className="num">In flight</th>
                  <th className="num">Per second</th>
                  <th className="num">Random delay (ms)</th>
                  <th>Back off on 429/503</th>
                  <th>Now</th>
                  <th aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {draft.pools.map((p, i) => {
                  const s = live(p.name);
                  return (
                    <tr key={i}>
                      <td>
                        <input
                          aria-label="Pool name"
                          value={p.name}
                          maxLength={60}
                          onChange={(e) => setDraft(renamePool(draft, i, e.target.value))}
                        />
                      </td>
                      <td className="num">
                        <input
                          type="number"
                          aria-label="Requests in flight"
                          min={1}
                          max={200}
                          value={p.concurrency}
                          onChange={(e) => update(i, { concurrency: Number(e.target.value) })}
                        />
                      </td>
                      <td className="num">
                        <input
                          type="number"
                          aria-label="Requests per second (0 = no limit)"
                          min={0}
                          max={1000}
                          value={p.requestsPerSecond}
                          onChange={(e) => update(i, { requestsPerSecond: Number(e.target.value) })}
                        />
                      </td>
                      <td className="num">
                        <input
                          type="number"
                          aria-label="Random delay in milliseconds"
                          min={0}
                          max={60000}
                          value={p.randomDelayMs}
                          onChange={(e) => update(i, { randomDelayMs: Number(e.target.value) })}
                        />
                      </td>
                      <td>
                        <Switch
                          label={p.backOff ? "on" : "off"}
                          checked={p.backOff}
                          onChange={(backOff) => update(i, { backOff })}
                        />
                      </td>
                      <td className="muted">
                        {s
                          ? `${s.inFlight} in flight · ${s.waiting} waiting · ${s.sent} sent` +
                            (s.throttled ? ` · backed off ${s.throttled}×` : "") +
                            (s.pausedMs ? ` · paused ${Math.ceil(s.pausedMs / 1000)} s` : "")
                          : "not saved yet"}
                      </td>
                      <td>
                        <IconButton
                          icon="trash"
                          label={`Delete pool ${p.name}`}
                          onClick={() => setDraft(removePool(draft, i))}
                        />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            <div>
              <Button
                size="sm"
                variant="ghost"
                icon="plus"
                disabled={draft.pools.length >= 50}
                onClick={() =>
                  setDraft({
                    ...draft,
                    pools: [...draft.pools, newPool(draft.pools.map((p) => p.name))],
                  })
                }
              >
                Add pool
              </Button>
            </div>
            <h4>Which tool uses which pool</h4>
            <div className="pool-assignments">
              {view.tools.map((t) => (
                <label key={t.id} className="inline-field">
                  {t.label}
                  <select
                    value={draft.assignments[t.id] ?? ""}
                    onChange={(e) => {
                      const assignments = Object.fromEntries(
                        Object.entries(draft.assignments).filter(([tool]) => tool !== t.id),
                      );
                      if (e.target.value) assignments[t.id] = e.target.value;
                      setDraft({ ...draft, assignments });
                    }}
                  >
                    <option value="">Own limits only</option>
                    {draft.pools.map((p, i) => (
                      <option key={i} value={p.name}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                </label>
              ))}
            </div>
            <p className="muted">
              Parallel Repeater group sends (synchronised on the last byte) and the browser crawl's
              page loads do not wait for pools.
            </p>
          </>
        )}
        {error && draft && <p className="callout callout-danger">{error}</p>}
        <div className="form-actions">
          <span className="spacer" />
          <Button variant="ghost" onClick={onClose}>
            Close
          </Button>
          <Button variant="primary" disabled={busy || !draft} onClick={() => void save()}>
            Save
          </Button>
        </div>
      </div>
    </div>
  );
}
