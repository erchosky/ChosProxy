import { useEffect, useState } from "react";
import type { ToolConfig } from "../bindings/ToolConfig";
import { type ConfigTool, loadableSettings, settingsOf } from "../lib/toolConfigs";
import { api, dialogs, toIpcError } from "../services/ipc";
import { useAppStore } from "../state/app";
import { Button } from "./Button";

/**
 * The configuration library of a tool (M31b): load saved settings into
 * the form, save the form's settings under a name, delete, import and
 * export. Loading changes the form only; nothing starts.
 */
export function ConfigLibraryBar({
  tool,
  current,
  onLoad,
}: {
  tool: ConfigTool;
  /** The form's configuration now. */
  current: object;
  /** Puts saved settings into the form. */
  onLoad(patch: Record<string, unknown>): void;
}) {
  const [list, setList] = useState<ToolConfig[]>([]);
  const [selected, setSelected] = useState("");
  const [name, setName] = useState("");
  const notify = useAppStore((s) => s.notify);

  useEffect(() => {
    let alive = true;
    api.toolConfigs(tool).then(
      (l) => alive && setList(l),
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, [tool]);

  const run = async (action: () => Promise<void>) => {
    try {
      await action();
    } catch (e) {
      notify("error", toIpcError(e).message);
    }
  };

  function load(which: string) {
    setSelected(which);
    const saved = list.find((c) => c.name === which);
    if (!saved) return;
    const patch = loadableSettings(tool, current, saved.json);
    if (patch === null) return notify("error", "That configuration cannot be read.");
    onLoad(patch);
    setName(saved.name);
    notify("info", `Settings “${saved.name}” loaded; check them before starting.`);
  }

  return (
    <div className="config-library" role="group" aria-label="Saved settings">
      <select aria-label="Saved settings" value={selected} onChange={(e) => load(e.target.value)}>
        <option value="">Saved settings…</option>
        {list.map((c) => (
          <option key={c.name} value={c.name}>
            {c.name}
          </option>
        ))}
      </select>
      <input
        aria-label="Name to save the settings under"
        placeholder="Save as…"
        maxLength={80}
        value={name}
        onChange={(e) => setName(e.target.value)}
      />
      <Button
        size="sm"
        variant="ghost"
        disabled={name.trim() === ""}
        title="Save these settings (not the target) under this name"
        onClick={() =>
          void run(async () => {
            const saved = await api.saveToolConfig({
              tool,
              name: name.trim(),
              json: JSON.stringify(settingsOf(tool, current)),
            });
            setList(saved);
            setSelected(name.trim());
            notify("info", "Settings saved.");
          })
        }
      >
        Save
      </Button>
      <Button
        size="sm"
        variant="ghost"
        disabled={!list.some((c) => c.name === selected)}
        onClick={() =>
          void run(async () => {
            setList(await api.deleteToolConfig(tool, selected));
            setSelected("");
          })
        }
      >
        Delete
      </Button>
      <Button
        size="sm"
        variant="ghost"
        onClick={() =>
          void run(async () => {
            const path = await dialogs.chooseJsonFile();
            if (path) setList(await api.importToolConfigs(path, tool));
          })
        }
      >
        Import…
      </Button>
      <Button
        size="sm"
        variant="ghost"
        disabled={list.length === 0}
        onClick={() =>
          void run(async () => {
            const path = await dialogs.chooseJsonDestination(`${tool}-settings.json`);
            if (path) await api.exportToolConfigs(path, tool);
          })
        }
      >
        Export…
      </Button>
    </div>
  );
}
