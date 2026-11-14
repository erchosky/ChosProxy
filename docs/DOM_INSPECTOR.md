# DOM Inspector

Finds client-side flaws while you use an application in the lab browser:
places where values from the URL, the referrer, `window.name`, cookies or
`postMessage` reach code that writes HTML, runs code or navigates.

Only use it on applications you are authorized to test.

## Start

1. **Tools → DOM Inspector → Open lab browser with DOM Inspector**
   (optionally with a start URL). It needs a Chromium-family browser:
   Chromium, Chrome, Edge or Brave (`sudo apt install chromium` on Kali).
2. A **canary** is shown, a unique word such as `chos2k7…`. Put it where the
   page reads input: the fragment (`#chos…`), a parameter (`?q=chos…`),
   `window.name`, or a message. The **+ canary in #** and **+ canary in ?**
   buttons add it to the URL field; **Open in the lab browser** opens that
   URL in the instrumented tab.
3. Use the page. Observations appear live.

## What it reports

| Kind | Example | Finding |
|---|---|---|
| Source → sink | `location.hash` → `element.innerHTML` | DOM-based XSS candidate (High/Firm with the canary, Medium/Tentative otherwise) |
| Navigation | `location.search` → navigation, `anchor.href`, `window.open` | URL injection or open redirection |
| postMessage listener | a `message` listener whose code never mentions `origin` | Low, tentative |
| Prototype pollution | `?__proto__[chos…]=1` adds `Object.prototype.chos…` | Medium |
| DOM clobbering | `<img name="referrer">` shadows `document.referrer` | Info |

Each observation shows **Source → Transformation → Sink**: the source and
its value, whether the value was URL-decoded on the way, the value at the
sink with the match highlighted, the code location (`file:line:col`) and
the JavaScript stack. Repeats are merged and counted. Findings are created
only when the page's response is in the history (it is the evidence).

Watched sinks: `innerHTML`, `outerHTML`, `insertAdjacentHTML`,
`document.write(ln)`, `createContextualFragment`, `iframe.srcdoc/src`,
`script.src/text`, `anchor.href`, `form.action`, dangerous attributes via
`setAttribute` (event handlers, `href`, `src`, `srcdoc`, `action`), string
`setTimeout`/`setInterval`, the `Function` constructor, `window.open` and
navigations.

## Try it

The local lab server has intentionally vulnerable pages at
`http://127.0.0.1:9090/dom/` (`hash`, `search`, `redirect`, `message`,
`pollution`, `clobber`).

## How it works and its limits

The lab browser is started with its DevTools endpoint on a random loopback
port. ChosProxy installs a small script in every frame before the page's
scripts; it only observes and reports through a DevTools binding, never the
network (ADR-0027).

* While it runs, any local process could connect to that port and control
  the lab browser. Run it on your own machine, and close the browser when
  you are done.
* Direct `eval` is not hooked: wrapping it would change how pages behave.
  Assignments to `location` cannot be hooked, so navigations are observed
  instead.
* Page code runs next to the script and could send fake reports. Treat
  findings as leads to confirm.
* Firefox is not supported yet.
