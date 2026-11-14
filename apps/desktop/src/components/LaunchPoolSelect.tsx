import { useEffect, useState } from "react";
import { api } from "../services/ipc";

/**
 * Chooses the resource pool a task sends through from its first request
 * ("" = the pool assigned to its tool). Hidden while no pool exists.
 */
export function LaunchPoolSelect({
  value,
  onChange,
}: {
  value: string;
  onChange(pool: string): void;
}) {
  const [names, setNames] = useState<string[]>([]);
  useEffect(() => {
    let alive = true;
    api
      .resourcePools()
      .then((view) => {
        if (alive) setNames(view.pools.map((p) => p.config.name));
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);
  if (names.length === 0) return null;
  return (
    <label className="inline-field" title="The resource pool this task sends through">
      Resource pool
      <select value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">the tool&apos;s pool</option>
        {names.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </select>
    </label>
  );
}
