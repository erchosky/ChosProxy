/* eslint-disable no-eval, no-restricted-syntax, no-implied-eval, no-new-func,
   @typescript-eslint/no-dynamic-delete --
   This test drives the DOM sinks the instrumentation watches (innerHTML,
   eval of the injected script); it is not UI code (ADR-0007 rule). */
// The DOM Inspector instrumentation (crates/dom/src/inspector.js) running
// in jsdom: it must report source → sink flows, leave page behaviour alone
// and stay quiet when nothing flows.
import inspectorSource from "../../../../crates/dom/src/inspector.js?raw";
import { beforeAll, describe, expect, it } from "vitest";

const CANARY = "chostestcanary42";
const BINDING = "__chosproxy_testbinding";

interface Report {
  kind: string;
  sink: string;
  value: string;
  source: string | null;
  matched: string | null;
  transformation: string | null;
  canary: boolean;
  checksOrigin?: boolean;
  stack: string[];
}

const reports: Report[] = [];
const g = globalThis as unknown as Record<string, unknown>;

beforeAll(() => {
  const source = inspectorSource
    .replaceAll("__CHOSPROXY_BINDING__", BINDING)
    .replaceAll("__CHOSPROXY_CANARY__", CANARY);
  g[BINDING] = (json: string) => reports.push(JSON.parse(json) as Report);
  // Indirect eval: runs in the global scope like an injected script.
  (0, eval)(source);
});

const last = () => reports[reports.length - 1];

describe("DOM Inspector instrumentation", () => {
  it("removes its binding from the page", () => {
    expect(BINDING in g).toBe(false);
  });

  it("reports the canary reaching innerHTML with its source", () => {
    window.location.hash = `#name=${CANARY}`;
    const div = document.createElement("div");
    div.innerHTML = `<b>${CANARY}</b>`;
    expect(div.innerHTML).toBe(`<b>${CANARY}</b>`);
    expect(last()).toMatchObject({
      kind: "flow",
      sink: "element.innerHTML",
      source: "location.hash",
      matched: CANARY,
      canary: true,
    });
    expect(Array.isArray(last()?.stack)).toBe(true);
  });

  it("matches pieces of source values without the canary", () => {
    window.history.replaceState(null, "", "/page?q=hello%20world123&x=1");
    const before = reports.length;
    document.body.insertAdjacentHTML("beforeend", "<p>hello world123</p>");
    expect(reports.length).toBe(before + 1);
    expect(last()).toMatchObject({
      sink: "element.insertAdjacentHTML",
      source: "location.search",
      matched: "hello world123",
      transformation: "URL-decoded",
      canary: false,
    });
  });

  it("stays quiet for values unrelated to any source", () => {
    const before = reports.length;
    const div = document.createElement("div");
    div.innerHTML = "<i>static template</i>";
    div.setAttribute("class", CANARY); // not a dangerous attribute
    expect(reports.length).toBe(before);
  });

  it("watches dangerous attributes and string timers", () => {
    const a = document.createElement("a");
    a.setAttribute("onclick", `alert('${CANARY}')`);
    expect(last()).toMatchObject({ sink: "element.setAttribute", canary: true });
    try {
      // Node's timers reject strings; the report is made before the call.
      window.clearTimeout(window.setTimeout(`void "${CANARY}"` as unknown as () => void, 100000));
    } catch {
      /* jsdom/Node only */
    }
    expect(last()).toMatchObject({ sink: "setTimeout(string)" });
  });

  it("de-duplicates identical reports", () => {
    const div = document.createElement("div");
    div.innerHTML = `<u>${CANARY}</u>`;
    const count = reports.length;
    div.innerHTML = `<u>${CANARY}</u>`;
    expect(reports.length).toBe(count);
  });

  it("reports message listeners and whether they check the origin", () => {
    window.addEventListener("message", function noCheck(e: MessageEvent) {
      document.title = String(e.data);
    });
    expect(last()).toMatchObject({ kind: "messageListener", checksOrigin: false });
    window.addEventListener("message", (e: MessageEvent) => {
      if (e.origin !== "https://trusted.example") return;
    });
    expect(last()).toMatchObject({ kind: "messageListener", checksOrigin: true });
  });

  it("keeps the Function constructor working", () => {
    const fn = new Function("a", "return a + 1") as (a: number) => number;
    expect(fn(1)).toBe(2);
    expect((() => 0) instanceof Function).toBe(true);
    Function(`return "${CANARY}"`);
    expect(last()).toMatchObject({ sink: "Function()", canary: true });
  });

  it("notices prototype pollution", () => {
    const proto = Object.prototype as Record<string, unknown>;
    proto[CANARY] = "polluted";
    window.dispatchEvent(new Event("load"));
    delete proto[CANARY];
    expect(reports.some((r) => r.kind === "prototypePollution" && r.canary)).toBe(true);
  });

  // DOM clobbering needs document named properties, which jsdom does not
  // implement; it is checked in Chromium (docs/DOM_INSPECTOR.md).
});
