import { useState } from "react";
import { Button } from "../../components/Button";
import { api, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import { useTasksStore } from "../../state/tasks";

const REPEATS: { minutes: number; label: string }[] = [
  { minutes: 0, label: "Once" },
  { minutes: 60, label: "Every hour" },
  { minutes: 1440, label: "Every day" },
  { minutes: 10080, label: "Every week" },
];

/** `YYYY-MM-DDTHH:MM` in local time, for datetime-local inputs. */
export function localInput(ms: number): string {
  const d = new Date(ms - new Date(ms).getTimezoneOffset() * 60000);
  return d.toISOString().slice(0, 16);
}

/** "Schedule…" for the crawl, discovery and scan dialogs. */
export function ScheduleControls({
  kind,
  name,
  config,
  onScheduled,
}: {
  kind: "crawl" | "discovery" | "scan";
  name: string;
  /** The settings as they would be started now. */
  config(): unknown;
  onScheduled(): void;
}) {
  const [open, setOpen] = useState(false);
  const [when, setWhen] = useState(() => localInput(Date.now() + 3600_000));
  const [repeat, setRepeat] = useState(0);
  const [error, setError] = useState<string | null>(null);

  async function schedule() {
    const at = new Date(when).getTime();
    if (Number.isNaN(at)) {
      setError("choose a date and time");
      return;
    }
    try {
      const s = await api.scheduleCreate({
        kind,
        name,
        config: JSON.stringify(config()),
        nextRun: at,
        repeatMinutes: repeat,
      });
      useAppStore
        .getState()
        .notify("info", `Scheduled “${s.name}” for ${new Date(s.nextRun).toLocaleString()}`);
      void useTasksStore.getState().refresh();
      onScheduled();
    } catch (e) {
      setError(toIpcError(e).message);
    }
  }

  if (!open)
    return (
      <Button variant="ghost" icon="restart" onClick={() => setOpen(true)}>
        Schedule…
      </Button>
    );
  return (
    <span className="schedule-controls">
      <input
        type="datetime-local"
        aria-label="Run at"
        value={when}
        onChange={(e) => setWhen(e.target.value)}
      />
      <select
        aria-label="Repeat"
        value={repeat}
        onChange={(e) => setRepeat(Number(e.target.value))}
      >
        {REPEATS.map((r) => (
          <option key={r.minutes} value={r.minutes}>
            {r.label}
          </option>
        ))}
      </select>
      <Button variant="primary" onClick={() => void schedule()}>
        Schedule
      </Button>
      {error && <span className="field-error">{error}</span>}
    </span>
  );
}
