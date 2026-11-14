import { useEffect, useMemo, useState } from "react";
import type { AgentApproval } from "../../bindings/AgentApproval";
import type { AiActivity } from "../../bindings/AiActivity";
import type { TaskStatus } from "../../bindings/TaskStatus";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Markdown } from "../../components/Markdown";
import { EmptyState, ErrorState } from "../../components/States";
import { Tabs } from "../../components/Tabs";
import { formatClock, formatDateTime } from "../../lib/format";
import { api, toIpcError } from "../../services/ipc";
import { parseIds } from "../../state/ai";
import { transcript, useAgentStore, interrupted } from "../../state/agent";
import { useAppStore } from "../../state/app";

const STATUS_TONE: Record<TaskStatus, "ok" | "warn" | "danger" | "neutral" | "accent"> = {
  queued: "neutral",
  running: "accent",
  paused: "warn",
  completed: "ok",
  failed: "danger",
  cancelled: "neutral",
};

type Tab = "tasks" | "activity";

/** ChosAgent (M30): agent tasks with approvals, and the AI activity log. */
export function AgentView() {
  const [tab, setTab] = useState<Tab>("tasks");
  const load = useAgentStore((s) => s.load);
  const pending = useAgentStore((s) => s.approvals.length);
  useEffect(() => {
    void load();
  }, [load]);
  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1>ChosAgent</h1>
          <p className="page-sub">
            An agent that works on a goal with ChosProxy&apos;s own tools. What each tool may do is
            set in Settings → ChosAgent; sending a request always waits for your approval, only to
            targets in scope.
          </p>
        </div>
      </div>
      <Tabs<Tab>
        items={[
          { id: "tasks", label: pending ? `Tasks (${pending} waiting)` : "Tasks" },
          { id: "activity", label: "AI activity log" },
        ]}
        value={tab}
        onChange={setTab}
        label="ChosAgent"
      />
      {tab === "tasks" ? <TasksTab /> : <ActivityTab />}
    </div>
  );
}

function TasksTab() {
  return (
    <div className="agent-layout">
      <div className="agent-side">
        <NewTask />
        <TaskList />
      </div>
      <TaskPanel />
    </div>
  );
}

function NewTask() {
  const [goal, setGoal] = useState("");
  const draftIds = useAgentStore((s) => s.draftIds);
  const setDraftIds = useAgentStore((s) => s.setDraftIds);
  const [busy, setBusy] = useState(false);
  const notify = useAppStore((s) => s.notify);

  async function start() {
    setBusy(true);
    try {
      const task = await api.agentStart({
        goal,
        attachments: parseIds(draftIds).map((id) => ({ kind: "exchange" as const, id })),
      });
      setGoal("");
      setDraftIds("");
      await useAgentStore.getState().load();
      await useAgentStore.getState().select(task.id);
    } catch (e) {
      notify("error", toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card" aria-label="New agent task">
      <h2>New task</h2>
      <label className="field-stack">
        Goal
        <textarea
          rows={3}
          value={goal}
          placeholder="e.g. Summarise how this app authenticates and which endpoints lack security headers."
          onChange={(e) => setGoal(e.target.value)}
        />
      </label>
      <label className="field-stack">
        Attach requests (optional)
        <input
          className="mono"
          placeholder="e.g. 12, 14-16"
          value={draftIds}
          onChange={(e) => setDraftIds(e.target.value)}
        />
      </label>
      <div className="form-actions">
        <Button
          variant="primary"
          size="sm"
          disabled={busy || goal.trim() === ""}
          onClick={() => void start()}
        >
          {busy ? "Starting…" : "Start"}
        </Button>
        <span className="muted">Uses the ChosAI model (Settings → ChosAI).</span>
      </div>
    </section>
  );
}

function TaskList() {
  const tasks = useAgentStore((s) => s.tasks);
  const selected = useAgentStore((s) => s.selected);
  const approvals = useAgentStore((s) => s.approvals);
  if (tasks.length === 0) return <p className="muted">No agent tasks yet.</p>;
  return (
    <ul className="agent-task-list" aria-label="Agent tasks">
      {tasks.map((t) => {
        const waiting = approvals.filter((a) => a.taskId === t.id).length;
        return (
          <li key={t.id}>
            <button
              type="button"
              className="agent-task"
              aria-current={selected === t.id ? "true" : undefined}
              onClick={() => void useAgentStore.getState().select(t.id)}
            >
              <span className="agent-task-name">
                #{t.id} {t.name.replace(/^Agent: /, "")}
              </span>
              <span className="agent-task-meta">
                <Badge tone={STATUS_TONE[t.status]}>{t.status}</Badge>
                {waiting > 0 && <Badge tone="warn">{waiting} waiting</Badge>}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function TaskPanel() {
  const tasks = useAgentStore((s) => s.tasks);
  const selected = useAgentStore((s) => s.selected);
  const activity = useAgentStore((s) => s.activity);
  const state = useAgentStore((s) => s.state);
  const error = useAgentStore((s) => s.error);
  const notify = useAppStore((s) => s.notify);
  const [message, setMessage] = useState("");
  const task = tasks.find((t) => t.id === selected);
  const items = useMemo(() => transcript(activity), [activity]);

  if (error) return <ErrorState title="ChosAgent" message={error} />;
  if (!task)
    return (
      <EmptyState title="No task selected">
        Start a task with a goal; the agent plans, reads the project and asks before it acts.
      </EmptyState>
    );

  const control = async (fn: (id: number) => Promise<null>) => {
    try {
      await fn(task.id);
      await useAgentStore.getState().load();
    } catch (e) {
      notify("error", toIpcError(e).message);
    }
  };

  async function send() {
    if (!task) return;
    try {
      await api.agentMessage(task.id, message);
      setMessage("");
      await useAgentStore.getState().refresh();
    } catch (e) {
      notify("error", toIpcError(e).message);
    }
  }

  async function continueTask() {
    if (!task) return;
    try {
      const next = await api.agentContinue(task.id, message);
      setMessage("");
      await useAgentStore.getState().load();
      await useAgentStore.getState().select(next.id);
    } catch (e) {
      notify("error", toIpcError(e).message);
    }
  }

  const live = task.status === "running" || task.status === "paused" || task.status === "queued";
  return (
    <section className="card agent-panel" aria-label={`Agent task ${task.id}`}>
      <div className="dialog-row">
        <strong>
          #{task.id} {task.name.replace(/^Agent: /, "")}
        </strong>
        <Badge tone={STATUS_TONE[task.status]}>{task.status}</Badge>
        {state.running && (
          <span className="muted">
            {state.steps} step(s) · {state.sends} request(s) sent
          </span>
        )}
        <span className="spacer" />
        {task.status === "running" && (
          <Button size="sm" variant="ghost" onClick={() => void control(api.taskPause)}>
            Pause
          </Button>
        )}
        {task.status === "paused" && (
          <Button size="sm" variant="ghost" onClick={() => void control(api.taskResume)}>
            Resume
          </Button>
        )}
        {live && (
          <Button size="sm" variant="ghost" onClick={() => void control(api.taskCancel)}>
            Stop
          </Button>
        )}
      </div>
      {task.lastError && <p className="callout callout-danger">{task.lastError}</p>}
      {state.plan.length > 0 && (
        <div className="agent-plan">
          <strong>Plan</strong>
          <ul>
            {state.plan.map((s, i) => (
              <li key={i} className={s.done ? "agent-step-done" : undefined}>
                <span aria-hidden="true">{s.done ? "✓" : "○"}</span> {s.title}
              </li>
            ))}
          </ul>
        </div>
      )}
      {state.approvals.map((a) => (
        <ApprovalCard key={a.id} approval={a} />
      ))}
      <ol className="agent-transcript" aria-label="Transcript">
        {items.map((it) => (
          <li key={it.id} className={`agent-item agent-item-${it.kind}`}>
            <div className="agent-item-head">
              <strong>{it.title}</strong>
              {it.permission && <Badge>{it.permission}</Badge>}
              {it.decision && (
                <Badge
                  tone={
                    it.decision === "ran" || it.decision.startsWith("approved")
                      ? "ok"
                      : it.decision.startsWith("rejected")
                        ? "warn"
                        : "danger"
                  }
                >
                  {it.decision}
                </Badge>
              )}
              <span className="spacer" />
              <span className="muted">{formatClock(it.at)}</span>
            </div>
            {it.kind === "answer" || it.kind === "thinking" ? (
              <Markdown source={it.body} />
            ) : (
              it.body && <p className="agent-item-body">{it.body}</p>
            )}
          </li>
        ))}
      </ol>
      {state.queued.length > 0 && (
        <p className="muted">
          Waiting to be read by the agent: {state.queued.map((q) => `“${q}”`).join(", ")}
        </p>
      )}
      {!live && !state.running && (
        <div className="agent-continue">
          {interrupted(task, activity) && (
            <p className="callout callout-warn">
              Interrupted: this task was running when the project closed. Nothing restarts by
              itself; you can continue it in a new task that starts from this transcript.
            </p>
          )}
          <div className="dialog-row">
            <input
              className="agent-message"
              placeholder="Continue in a new task: anything to add (optional)…"
              value={message}
              onChange={(e) => setMessage(e.target.value)}
            />
            <Button size="sm" onClick={() => void continueTask()}>
              Continue in a new task
            </Button>
          </div>
        </div>
      )}
      {state.running && (
        <div className="dialog-row">
          <input
            className="agent-message"
            placeholder="Message the agent (read before its next step)…"
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && message.trim() && void send()}
          />
          <Button size="sm" disabled={message.trim() === ""} onClick={() => void send()}>
            Send
          </Button>
        </div>
      )}
      <p className="muted">
        Written by a language model: check its answer against the traffic before relying on it.
      </p>
    </section>
  );
}

function ApprovalCard({ approval }: { approval: AgentApproval }) {
  const [note, setNote] = useState("");
  const notify = useAppStore((s) => s.notify);
  async function decide(approve: boolean) {
    try {
      await api.agentDecide(approval.id, approve, note);
      await useAgentStore.getState().refresh();
    } catch (e) {
      notify("error", toIpcError(e).message);
    }
  }
  return (
    <section className="agent-approval" aria-label="Waiting for your approval">
      <div className="dialog-row">
        <Badge tone="warn">
          {approval.kind === "outgoing" ? "send to the external model?" : "approve?"}
        </Badge>
        <strong>{approval.summary}</strong>
      </div>
      {approval.target && <p className="muted">Target: {approval.target}</p>}
      {approval.reason && (
        <p>
          <span className="muted">The agent&apos;s reason:</span> {approval.reason}
        </p>
      )}
      <pre className="mono agent-approval-detail">{approval.detail}</pre>
      <div className="dialog-row">
        <input
          placeholder="Note for the agent (optional)"
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
        <span className="spacer" />
        <Button size="sm" variant="ghost" onClick={() => void decide(false)}>
          Reject
        </Button>
        <Button size="sm" variant="primary" onClick={() => void decide(true)}>
          {approval.kind === "outgoing" ? "Send" : "Approve"}
        </Button>
      </div>
    </section>
  );
}

function ActivityTab() {
  const [rows, setRows] = useState<AiActivity[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<number | null>(null);
  const version = useAgentStore((s) => s.activity.length + s.approvals.length);
  useEffect(() => {
    let alive = true;
    api.aiActivity(null, 0, 2000).then(
      (r) => alive && setRows(r.reverse()),
      (e: unknown) => alive && setError(toIpcError(e).message),
    );
    return () => {
      alive = false;
    };
  }, [version]);
  if (error) return <ErrorState title="Could not read the AI activity log" message={error} />;
  if (!rows) return null;
  if (rows.length === 0)
    return (
      <EmptyState title="No AI activity yet">
        ChosAI questions and every model call, tool call and decision of ChosAgent are recorded
        here, in the project.
      </EmptyState>
    );
  return (
    <div className="agent-activity">
      <table className="endpoint-table" aria-label="AI activity log">
        <thead>
          <tr>
            <th>Time</th>
            <th>Task</th>
            <th>Action</th>
            <th>Tool</th>
            <th>Target</th>
            <th>Permission</th>
            <th>Decision</th>
            <th>Result</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr
              key={r.id}
              className="clickable"
              onClick={() => setOpen(open === r.id ? null : r.id)}
            >
              <td className="mono">{formatDateTime(r.at)}</td>
              <td>{r.taskId === null ? "ChosAI" : `#${r.taskId}`}</td>
              <td>{r.action}</td>
              <td className="mono">{r.tool}</td>
              <td className="mono">{r.target}</td>
              <td>{r.permission}</td>
              <td>{r.decision}</td>
              <td>
                {r.result}
                {open === r.id && r.detail && (
                  <pre className="mono agent-approval-detail">{r.detail}</pre>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
