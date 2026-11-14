import { describe, expect, it, vi } from "vitest";
import { deltaBuffer, newStreamId, parseIds, sessionRuleFrom, tasksFor } from "./ai";

describe("ChosAI helpers", () => {
  it("offers tasks that fit what is attached", () => {
    expect(tasksFor([{ kind: "finding", id: 1 }])).toEqual([
      "explainFinding",
      "draftFinding",
      "ask",
    ]);
    expect(tasksFor([{ kind: "exchange", id: 1 }])).toContain("loginFlow");
    expect(tasksFor([{ kind: "text", label: "x", text: "y" }])).toEqual([
      "explainRequest",
      "explainJavascript",
      "ask",
    ]);
  });

  it("reads exchange ids and ranges", () => {
    expect(parseIds("12, #14 16-18 x 12")).toEqual([12, 14, 16, 17, 18]);
    expect(parseIds("1-100")).toHaveLength(20);
    expect(parseIds("")).toEqual([]);
  });

  it("reads the session rule a login-flow answer ends with", () => {
    const answer = [
      "The login is #12.",
      "```chosproxy-session-rule",
      JSON.stringify({
        name: "Lab login",
        hosts: ["lab.example"],
        macroRequests: ["#12", 13, -1],
        expiredStatuses: [401, 9999],
        extract: [{ name: "csrf", regex: "csrf=(\\w+)" }, { name: 1 }],
        headers: [{ name: "X-CSRF", value: "{{csrf}}" }],
      }),
      "```",
    ].join("\n");
    const rule = sessionRuleFrom(answer);
    expect(rule).toMatchObject({
      enabled: true,
      name: "Lab login",
      hosts: ["lab.example"],
      tools: [],
      useCookieJar: true,
      macroRequests: [12, 13],
      expiredStatuses: [401],
      extract: [{ name: "csrf", regex: "csrf=(\\w+)" }],
      headers: [{ name: "X-CSRF", value: "{{csrf}}" }],
    });
    expect(sessionRuleFrom("no rule here")).toBeNull();
    expect(sessionRuleFrom("```chosproxy-session-rule\nnot json\n```")).toBeNull();
    expect(sessionRuleFrom("```chosproxy-session-rule\n[1]\n```")).toBeNull();
  });

  it("shows a streamed answer at most once per interval", () => {
    vi.useFakeTimers();
    try {
      const shown: string[] = [];
      const buffer = deltaBuffer((t) => shown.push(t), 60);
      buffer.push("He");
      buffer.push("llo");
      expect(shown).toEqual([]);
      vi.advanceTimersByTime(60);
      expect(shown).toEqual(["Hello"]);
      buffer.push(" world");
      // The end shows what is left at once and stops the timer.
      expect(buffer.end()).toBe("Hello world");
      expect(shown).toEqual(["Hello", "Hello world"]);
      vi.advanceTimersByTime(200);
      expect(shown).toHaveLength(2);
      expect(newStreamId()).not.toBe(newStreamId());
    } finally {
      vi.useRealTimers();
    }
  });
});
