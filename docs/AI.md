# ChosAI

ChosAI asks a language model about the traffic and findings you choose, and
shows you its answer. It can:

* explain a request, a response, JavaScript or a finding;
* suggest manual checks and fixes;
* describe a login flow, with a session rule to keep tools logged in;
* draft report text.

**ChosAI only writes text.** It never sends requests, changes the project
or runs anything. It does not write exploit code. To let a model use
ChosProxy's tools, with your approval, see [ChosAgent](#chosagent) below.

## Turning it on

ChosAI is off until you choose a mode in Settings → ChosAI.

* **Local model**, the recommended mode. The model runs on this machine
  and nothing leaves it. The URL must be `127.0.0.1`, `::1` or `localhost`.
  Presets:

  | Server | API | URL |
  |---|---|---|
  | Ollama | Ollama | `http://127.0.0.1:11434` |
  | LM Studio | OpenAI-compatible | `http://127.0.0.1:1234` |
  | llama.cpp | OpenAI-compatible | `http://127.0.0.1:8080` |

  Then set the model name (for example `llama3.1`).
* **External provider.** Any service with an OpenAI-compatible
  `/v1/chat/completions` API, reached over https. Keep the API key in
  Settings → Credentials and choose it as the API key.

"Test the model" sends one tiny message, with no project data.

## What is sent

* Only the items you attach, never the project:
  * a request with its response;
  * a finding with its first piece of evidence;
  * the Repeater request;
  * a selection in a message viewer.
* Before anything is sent, these are redacted:
  * `Authorization`, `Proxy-Authorization` and API-key headers;
  * CSRF tokens;
  * cookie values;
  * JWTs;
  * parameters and JSON fields named like passwords, tokens, keys or
    session ids;
  * AWS, Google, GitHub, Slack and Stripe keys, and private keys;
  * your own regular expressions.

  The model sees `[REDACTED:kind]` in their place.
* The context is cut to a size limit (24 000 characters by default).

**Preview what is sent** shows the exact messages and what was hidden.
With an external provider the preview is required: the engine refuses an
external call unless it carries the digest of the context you previewed.
If you change anything, you preview again.

Calls are not stored in HTTP history. The project's audit log records each
question with its task, size and model, but not its content.

## Where to find it

* **HTTP history, Logger and other request lists:** right-click a request
  → "Explain with ChosAI…". "Explain a login flow" takes more request ids,
  for example `12, 14-16`.
* **Findings:** "ChosAI…" in the finding editor explains the finding or
  drafts report text.
* **Repeater:** the "ChosAI" button asks about the request in the editor.
* **Any message viewer:** select text, then "ChosAI".

## Conversations

After an answer, type a follow-up question below it. The earlier questions
and answers are sent again with it (your follow-ups are redacted too), so
with an external model the preview shows the whole conversation. Changing
the task, the question or the attached requests starts a new conversation.

## From a login flow to a session rule

"Explain a login flow" ends its answer with a session rule in a
`chosproxy-session-rule` block. **Review as session rule…** opens
Project → Session rules with that rule added below the others. Nothing is
saved until you check it and press Save.

Answers come from a language model. Check them against the traffic before
relying on them.

# ChosAgent

ChosAgent works on a goal you give it — "summarise how this app logs in",
"which endpoints in scope miss security headers?" — by calling ChosProxy's
own tools step by step, and ends with an answer. It uses the ChosAI model
(Settings → ChosAI).

## What it can use

Only these tools; it has no shell, files or other access:

| Tool | What it does | Default | Can be |
|---|---|---|---|
| `search_history` | search the HTTP history | Allow | Deny, Ask, Allow |
| `get_exchange` | read a request and response (redacted) | Allow | Deny, Ask, Allow |
| `list_findings`, `get_finding` | read the findings | Allow | Deny, Ask, Allow |
| `get_scope` | read the scope | Allow | Deny, Ask, Allow |
| `analyze_exchange` | run the passive checks on a stored request | Allow | Deny, Ask, Allow |
| `decode` | the Decoder | Allow | Deny, Ask, Allow |
| `update_plan` | keep the task's plan | Allow | Deny, Ask, Allow |
| `add_note` | set the note of a history entry | Ask | Deny, Ask, Allow |
| `draft_finding` | record a *tentative* finding for you to review | Ask | Deny, Ask, Allow |
| `propose_session_rule` | add a session rule | Ask | Deny, Ask |
| `send_request` | send one request to a target in scope | Ask | Deny, Ask |

Change them in Settings → ChosAgent, with the most model calls and
requests per task (25 and 20 by default).

* **Deny:** the call is refused and the agent is told so.
* **Ask:** the agent waits; you see exactly what it wants to run — for a
  request, the full request text and the agent's reason — and approve or
  reject it, with an optional note for the agent.
* **Allow:** it runs and is recorded.

ChosProxy enforces these itself; they do not depend on the model
following instructions. A tool name the agent makes up is refused.

**Sending requests** always asks — it cannot be set to Allow. It also needs
a scope (Target → Scope), and only targets inside it are accepted. Sent
requests go through resource pools and session rules (tool name `agent`)
and appear in HTTP history and the Logger with source `agent` and a note
naming the task. ChosAgent has no payload lists or active checks, and it is
told not to send attack payloads, brute force or load.

## Running a task

Open **ChosAgent** in the sidebar (or "Give to ChosAgent…" in a request
menu, which attaches that request) and write the goal. Then:

* the plan shows its steps as it works;
* calls waiting for you appear at the top (the sidebar shows how many);
* the transcript shows each step: what the agent said, each tool call with
  its permission and what happened;
* you can send it a message, read before its next step;
* Pause, Resume and Stop work as for other tasks. Stopping while it waits
  for you ends the task.

With an **external model**, each step also waits for you to approve the new
text that would be sent — on the first step, the whole context. What the
tools return is redacted with the ChosAI rules and cut to the context
limit before the agent sees it.

Models that cannot call tools natively (many small local models) are
given the tools in the prompt and answer with a fenced `tool` block;
ChosProxy switches to this when the server refuses tools.

## AI activity log

ChosAgent → **AI activity log** lists, for the project, every ChosAI
question (task, model, size — not the content) and everything agents did:
goals, model calls, tool calls with permission and decision, your
messages and approvals, answers, stops and errors. Click a row for its
detail. An agent task's rows are deleted with the task.

The automation API cannot start agents or approve their calls.

Answers come from a language model. Check them against the traffic before
relying on them.

## Streaming, Record login and continuing a task (M31b)

- Answers appear while the model writes them (Ollama and
  OpenAI-compatible servers). **Stop** ends the call; nothing of a stopped
  answer is kept or logged. A server that does not stream works as before.
- **Record login** (the login-flow task, or the palette command "Record a
  login for ChosAI"): press it, log in through the proxy, press **Stop
  recording**. The requests you made are attached (proxy traffic only,
  static files left out, at most 20). ChosProxy sends and replays nothing.
- **Continue in a new task** (ChosAgent, on a task that is not running):
  a new task with the same goal and items starts from a redacted summary
  of the old activity log, plus anything you add. An interrupted task
  never restarts by itself, and every approval is asked again.
