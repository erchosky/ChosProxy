import { useEffect, useRef, useState } from "react";
import type { AiAnswer } from "../../bindings/AiAnswer";
import type { AiMessage } from "../../bindings/AiMessage";
import type { AiPreview } from "../../bindings/AiPreview";
import type { AiRequest } from "../../bindings/AiRequest";
import type { AiTask } from "../../bindings/AiTask";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { Markdown } from "../../components/Markdown";
import { copyText } from "../../services/clipboard";
import { api, onAiDelta, toIpcError } from "../../services/ipc";
import { useAppStore } from "../../state/app";
import {
  type AiSubject,
  deltaBuffer,
  newStreamId,
  parseIds,
  sessionRuleFrom,
  TASK_LABELS,
  tasksFor,
  useAiStore,
  useSessionDraft,
} from "../../state/ai";

/** The ChosAI dialog (mounted once by the shell). */
export function AiDialog() {
  const subject = useAiStore((s) => s.subject);
  if (!subject) return null;
  return <Dialog key={JSON.stringify(subject)} subject={subject} />;
}

function Dialog({ subject }: { subject: AiSubject }) {
  const close = useAiStore((s) => s.close);
  const [task, setTask] = useState<AiTask>(subject.task);
  const [question, setQuestion] = useState("");
  const [extraIds, setExtraIds] = useState("");
  const [preview, setPreview] = useState<AiPreview | null>(null);
  const [answer, setAnswer] = useState<AiAnswer | null>(null);
  // The conversation after the first question: answers and follow-ups.
  const [turns, setTurns] = useState<AiMessage[]>([]);
  const [followUp, setFollowUp] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The answer while the model writes it, and the name of that call.
  const [partial, setPartial] = useState<string | null>(null);
  const stream = useRef<string | null>(null);
  // A login being recorded (AI-004): requests made through the proxy from
  // now on are attached when it stops.
  const [recording, setRecording] = useState(false);
  useEffect(() => {
    void api
      .loginRecording()
      .then((after) => setRecording(after !== null))
      .catch(() => undefined);
    // Closing the dialog stops an answer that is still being written.
    return () => {
      if (stream.current) void api.aiCancel(stream.current).catch(() => undefined);
    };
  }, []);

  async function toggleRecording() {
    setError(null);
    try {
      if (!recording) {
        await api.loginRecordingStart();
        setRecording(true);
        return;
      }
      const rec = await api.loginRecordingStop();
      setRecording(false);
      if (rec.ids.length === 0) {
        setError("No requests went through the proxy while recording.");
        return;
      }
      setExtraIds(rec.ids.join(", "));
      changed();
      useAppStore
        .getState()
        .notify(
          "info",
          `${rec.ids.length} request(s) attached` +
            (rec.skipped > 0 ? `; ${rec.skipped} left out (static files or too many)` : ""),
        );
    } catch (e) {
      setRecording(false);
      setError(toIpcError(e).message);
    }
  }

  const asking = answer !== null && followUp.trim() !== "";
  const request = (): AiRequest => ({
    task,
    question: question.trim() || null,
    attachments: [
      ...subject.attachments,
      ...parseIds(extraIds).map((id) => ({ kind: "exchange" as const, id })),
    ],
    conversation: asking ? [...turns, { role: "user", content: followUp.trim() }] : [],
  });

  async function showPreview() {
    setBusy(true);
    setError(null);
    try {
      setPreview(await api.aiPreview(request()));
    } catch (e) {
      setError(toIpcError(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function ask() {
    setBusy(true);
    setError(null);
    try {
      // An external model only gets a context the tester has seen.
      let approved: string | null = null;
      if (preview?.external) approved = preview.digest;
      else if (!preview) {
        const p = await api.aiPreview(request());
        if (p.external) {
          setPreview(p);
          setError("The model is outside this machine: check the context below, then send it.");
          return;
        }
      }
      const sent = request();
      const id = newStreamId();
      const buffer = deltaBuffer(setPartial);
      stream.current = id;
      setPartial("");
      const unlisten = await onAiDelta((d) => {
        if (d.stream === id) buffer.push(d.text);
      });
      let reply: AiAnswer;
      try {
        reply = await api.aiRunStream(sent, approved, id);
      } finally {
        unlisten();
        buffer.end();
        stream.current = null;
        setPartial(null);
      }
      setTurns([...sent.conversation, { role: "assistant", content: reply.text }]);
      setAnswer(reply);
      setFollowUp("");
      setPreview(null);
    } catch (e) {
      const err = toIpcError(e);
      setError(
        err.kind === "cancelled" ? "Stopped; nothing of that answer was kept." : err.message,
      );
    } finally {
      setBusy(false);
    }
  }

  // Anything changed after a preview needs a new one; changing the
  // question or the items starts a new conversation.
  const changed = () => {
    setPreview(null);
    setAnswer(null);
    setTurns([]);
    setFollowUp("");
  };
  const rule = answer ? sessionRuleFrom(answer.text) : null;
  const needsQuestion = task === "ask" && question.trim() === "";
  const nothingAttached =
    task === "loginFlow" && subject.attachments.length === 0 && parseIds(extraIds).length === 0;
  const tasks = [...new Set([subject.task, ...tasksFor(subject.attachments)])];

  return (
    <div className="palette-backdrop" role="presentation" onClick={close}>
      <div
        className="dialog dialog-form dialog-wide ai-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="ChosAI"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && close()}
      >
        <header className="dialog-head">
          <strong>ChosAI · {subject.title}</strong>
          <Button size="sm" variant="ghost" onClick={close}>
            Close
          </Button>
        </header>
        <div className="dialog-row">
          <label className="inline-field">
            Task
            <select
              value={task}
              onChange={(e) => {
                setTask(e.target.value as AiTask);
                changed();
              }}
            >
              {tasks.map((t) => (
                <option key={t} value={t}>
                  {TASK_LABELS[t]}
                </option>
              ))}
            </select>
          </label>
          {task === "loginFlow" && (
            <label className="inline-field">
              Also attach requests
              <input
                className="mono"
                placeholder="e.g. 12, 14-16"
                value={extraIds}
                onChange={(e) => {
                  setExtraIds(e.target.value);
                  changed();
                }}
              />
            </label>
          )}
          {task === "loginFlow" && (
            <Button
              size="sm"
              variant={recording ? "primary" : "ghost"}
              disabled={busy}
              onClick={() => void toggleRecording()}
            >
              {recording ? "Stop recording" : "Record login"}
            </Button>
          )}
        </div>
        {task === "loginFlow" && recording && (
          <p className="callout callout-warn">
            Recording: log in through the proxy now (the lab browser). ChosProxy sends nothing by
            itself; when you stop, the requests you made are attached here.
          </p>
        )}
        <label className="field-stack">
          {task === "ask" ? "Your question" : "Anything to add (optional)"}
          <textarea
            rows={2}
            value={question}
            placeholder={task === "ask" ? "What does this token do? Why is this response 403?" : ""}
            onChange={(e) => {
              setQuestion(e.target.value);
              changed();
            }}
          />
        </label>
        <div className="form-actions">
          <Button variant="ghost" disabled={busy} onClick={() => void showPreview()}>
            Preview what is sent
          </Button>
          <span className="spacer" />
          {answer === null && (
            <Button
              variant="primary"
              disabled={busy || needsQuestion || nothingAttached || recording}
              onClick={() => void ask()}
            >
              {busy ? "Asking…" : preview?.external ? "Send this context" : "Ask ChosAI"}
            </Button>
          )}
        </div>
        {turns.length > 1 && (
          <section className="ai-thread" aria-label="Conversation">
            {turns.slice(0, -1).map((t, i) =>
              t.role === "user" ? (
                <p key={i} className="ai-turn-user">
                  <strong>You:</strong> {t.content}
                </p>
              ) : (
                <div key={i} className="ai-turn-answer">
                  <Markdown source={t.content} />
                </div>
              ),
            )}
          </section>
        )}
        {partial !== null && (
          <section className="ai-answer" aria-label="Answer being written" aria-busy="true">
            <div className="dialog-row">
              <Badge>writing…</Badge>
              <span className="spacer" />
              <Button
                size="sm"
                variant="ghost"
                onClick={() => stream.current && void api.aiCancel(stream.current)}
              >
                Stop
              </Button>
            </div>
            <Markdown source={partial} />
          </section>
        )}
        {answer && partial === null && (
          <section className="ai-answer" aria-label="Answer">
            <div className="dialog-row">
              <Badge>{answer.model}</Badge>
              <span className="muted">
                {(answer.elapsedMs / 1000).toFixed(1)} s · {answer.sentChars.toLocaleString()}{" "}
                characters sent
              </span>
              <span className="spacer" />
              <Button
                size="sm"
                variant="ghost"
                icon="copy"
                onClick={() => {
                  void copyText(answer.text);
                  useAppStore.getState().notify("info", "Answer copied");
                }}
              >
                Copy
              </Button>
            </div>
            <Markdown source={answer.text} />
            {rule && (
              <div className="dialog-row">
                <Button
                  size="sm"
                  onClick={() => {
                    useSessionDraft.getState().offer(rule);
                    useAppStore.getState().setView("project");
                    close();
                  }}
                >
                  Review as session rule…
                </Button>
                <span className="muted">
                  Opens Project → Session rules with “{rule.name}” added; nothing is saved until you
                  save it.
                </span>
              </div>
            )}
            <p className="muted">
              Written by a language model: check it against the traffic before relying on it.
            </p>
          </section>
        )}
        {answer && (
          <div className="ai-follow-up">
            <label className="field-stack">
              Follow-up question
              <textarea
                rows={2}
                value={followUp}
                placeholder="Ask more about the same items; the conversation is sent again."
                onChange={(e) => {
                  setFollowUp(e.target.value);
                  setPreview(null);
                }}
              />
            </label>
            <div className="form-actions">
              <Button
                variant="ghost"
                disabled={busy || followUp.trim() === ""}
                onClick={() => void showPreview()}
              >
                Preview what is sent
              </Button>
              <span className="spacer" />
              <Button
                variant="primary"
                disabled={busy || followUp.trim() === ""}
                onClick={() => void ask()}
              >
                {busy ? "Asking…" : preview?.external ? "Send this context" : "Ask"}
              </Button>
            </div>
          </div>
        )}
        {error && <p className="callout callout-danger">{error}</p>}
        {preview && <PreviewPanel preview={preview} />}
      </div>
    </div>
  );
}

function PreviewPanel({ preview }: { preview: AiPreview }) {
  return (
    <section className="ai-preview" aria-label="Context preview">
      <div className="dialog-row">
        <Badge tone={preview.external ? "warn" : "ok"}>
          {preview.external ? "external model" : "on this machine"}
        </Badge>
        <span className="muted">{preview.target}</span>
        {!preview.enabled && <Badge tone="warn">ChosAI is off (Settings → ChosAI)</Badge>}
      </div>
      <p className="muted">
        {preview.context.redactions.length === 0
          ? "Nothing was redacted."
          : `Hidden: ${preview.context.redactions.map((r) => `${r.count} ${r.kind}`).join(", ")}.`}
        {preview.context.truncated && " Some text was cut to the context limit."}
      </p>
      <pre className="mono ai-preview-text">
        {preview.messages.map((m) => `### ${m.role}\n${m.content}`).join("\n\n")}
      </pre>
    </section>
  );
}
