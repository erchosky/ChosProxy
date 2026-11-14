// ChosProxy DOM Inspector (ADR-0027). Injected by ChosProxy into every
// frame of the lab browser before the page's own scripts. It only
// observes: every hooked API calls the original with the same arguments.
// Reports go to ChosProxy through a DevTools binding, never the network.
(() => {
  "use strict";
  const BINDING = "__CHOSPROXY_BINDING__";
  const CANARY = "__CHOSPROXY_CANARY__";
  const g = globalThis;
  if (Object.prototype.hasOwnProperty.call(g, "__chosproxyDomActive")) return;
  try {
    Object.defineProperty(g, "__chosproxyDomActive", { value: true });
  } catch (_) {
    return;
  }
  const send = g[BINDING];
  // The page cannot reach the binding afterwards (it runs after us).
  try {
    delete g[BINDING];
  } catch (_) {
    /* not configurable: harmless */
  }
  if (typeof send !== "function") return;

  // Originals, captured before any page script can replace them.
  const J = JSON.stringify;
  const now = Date.now;
  const ErrorCtor = Error;
  const strIncludes = String.prototype.includes;
  const strSlice = String.prototype.slice;
  const strSplit = String.prototype.split;
  const strToLower = String.prototype.toLowerCase;
  const decode = decodeURIComponent;
  const getOwnNames = Object.getOwnPropertyNames;
  const describe = Object.getOwnPropertyDescriptor;
  const define = Object.defineProperty;
  const apply = Reflect.apply;
  const construct = Reflect.construct;
  const fnToString = Function.prototype.toString;
  const MAX_VALUE = 2000;
  const MIN_TOKEN = 6;

  const has = (s, part) =>
    typeof s === "string" && apply(strIncludes, s, [part]);
  const cut = (s) =>
    typeof s === "string" && s.length > MAX_VALUE
      ? apply(strSlice, s, [0, MAX_VALUE])
      : String(s);
  const safeDecode = (s) => {
    try {
      return decode(s);
    } catch (_) {
      return s;
    }
  };
  const here = () => {
    try {
      return String(g.location && g.location.href);
    } catch (_) {
      return "";
    }
  };

  // Rate limit and de-duplication per frame.
  let budget = 50;
  let budgetStart = now();
  const seen = new Set();
  const report = (event) => {
    const t = now();
    if (t - budgetStart > 1000) {
      budget = 50;
      budgetStart = t;
    }
    const key =
      event.kind +
      "|" +
      event.sink +
      "|" +
      (event.matched || "") +
      "|" +
      cut(event.value);
    if (seen.has(key) || budget <= 0) return;
    if (seen.size > 2000) seen.clear();
    seen.add(key);
    budget -= 1;
    event.frameUrl = here();
    event.topFrame = g === g.top;
    event.at = t;
    try {
      event.stack = apply(strSplit, String(new ErrorCtor().stack || ""), [
        "\n",
      ]).slice(2, 22);
    } catch (_) {
      event.stack = [];
    }
    try {
      send(J(event));
    } catch (_) {
      /* the binding is gone: the inspector was detached */
    }
  };

  // ---------------------------------------------------------------- sources
  const recentMessages = [];
  const sources = () => {
    const out = [];
    const add = (name, value) => {
      if (typeof value === "string" && value.length >= 2)
        out.push([name, value]);
    };
    try {
      const l = g.location;
      add("location.hash", apply(strSlice, l.hash, [1]));
      add("location.search", apply(strSlice, l.search, [1]));
      add("location.pathname", l.pathname);
    } catch (_) {
      /* opaque origin */
    }
    try {
      add("document.referrer", g.document.referrer);
    } catch (_) {
      /* no document */
    }
    try {
      add("window.name", g.name);
    } catch (_) {
      /* cross-origin */
    }
    try {
      add("document.cookie", g.document.cookie);
    } catch (_) {
      /* sandboxed */
    }
    for (const m of recentMessages) add("postMessage data", m);
    return out;
  };

  // Splits a source value into the pieces an application typically uses.
  const tokens = (value) =>
    apply(strSplit, value, [/[&=?#/;,\s]+/]).filter(
      (t) => t.length >= MIN_TOKEN,
    );

  // How a sink value relates to the sources: the canary, or a piece of a
  // source value, raw or URL-decoded.
  const trace = (value) => {
    if (typeof value !== "string" || value.length === 0) return null;
    const all = sources();
    if (has(value, CANARY)) {
      let source = "canary";
      let sourceValue = null;
      let transformation = "unchanged";
      for (const [name, v] of all) {
        if (has(v, CANARY)) {
          source = name;
          sourceValue = v;
          break;
        }
        const d = safeDecode(v);
        if (has(d, CANARY)) {
          source = name;
          sourceValue = v;
          transformation = "URL-decoded";
          break;
        }
      }
      return {
        canary: true,
        source,
        sourceValue,
        matched: CANARY,
        transformation,
      };
    }
    for (const [name, v] of all) {
      for (const t of tokens(v)) {
        if (has(value, t)) {
          return {
            canary: false,
            source: name,
            sourceValue: v,
            matched: t,
            transformation: "unchanged",
          };
        }
        const d = safeDecode(t);
        if (d !== t && d.length >= MIN_TOKEN && has(value, d)) {
          return {
            canary: false,
            source: name,
            sourceValue: v,
            matched: d,
            transformation: "URL-decoded",
          };
        }
      }
    }
    return null;
  };

  const flow = (sink, value, kind) => {
    const text =
      typeof value === "string" ? value : value == null ? "" : String(value);
    const t = trace(text);
    if (!t) return;
    report({
      kind: kind || "flow",
      sink,
      value: cut(text),
      source: t.source,
      sourceValue: t.sourceValue === null ? null : cut(t.sourceValue),
      matched: t.matched,
      transformation: t.transformation,
      canary: t.canary,
    });
  };

  // ---------------------------------------------------------------- sinks
  const hookSetter = (proto, prop, sink) => {
    if (!proto) return;
    const d = describe(proto, prop);
    if (!d || typeof d.set !== "function" || !d.configurable) return;
    const original = d.set;
    define(proto, prop, {
      ...d,
      set(value) {
        try {
          flow(sink, value);
        } catch (_) {
          /* never break the page */
        }
        return apply(original, this, [value]);
      },
    });
  };
  const hookMethod = (owner, name, sink, pick) => {
    if (!owner) return;
    const original = owner[name];
    if (typeof original !== "function") return;
    const wrapper = {
      [name](...args) {
        try {
          const v = pick(args, this);
          if (v !== undefined) flow(sink, v);
        } catch (_) {
          /* never break the page */
        }
        return apply(original, this, args);
      },
    }[name];
    try {
      define(owner, name, {
        value: wrapper,
        writable: true,
        configurable: true,
        enumerable: false,
      });
    } catch (_) {
      /* frozen */
    }
  };

  const E = g.Element && g.Element.prototype;
  hookSetter(E, "innerHTML", "element.innerHTML");
  hookSetter(E, "outerHTML", "element.outerHTML");
  hookMethod(
    E,
    "insertAdjacentHTML",
    "element.insertAdjacentHTML",
    (a) => a[1],
  );
  hookMethod(
    g.Document && g.Document.prototype,
    "write",
    "document.write",
    (a) => a.join(""),
  );
  hookMethod(
    g.Document && g.Document.prototype,
    "writeln",
    "document.writeln",
    (a) => a.join(""),
  );
  hookMethod(
    g.Range && g.Range.prototype,
    "createContextualFragment",
    "range.createContextualFragment",
    (a) => a[0],
  );
  hookSetter(
    g.HTMLIFrameElement && g.HTMLIFrameElement.prototype,
    "srcdoc",
    "iframe.srcdoc",
  );
  hookSetter(
    g.HTMLIFrameElement && g.HTMLIFrameElement.prototype,
    "src",
    "iframe.src",
  );
  hookSetter(
    g.HTMLScriptElement && g.HTMLScriptElement.prototype,
    "src",
    "script.src",
  );
  hookSetter(
    g.HTMLScriptElement && g.HTMLScriptElement.prototype,
    "text",
    "script.text",
  );
  hookSetter(
    g.HTMLAnchorElement && g.HTMLAnchorElement.prototype,
    "href",
    "anchor.href",
  );
  hookSetter(
    g.HTMLFormElement && g.HTMLFormElement.prototype,
    "action",
    "form.action",
  );
  hookMethod(E, "setAttribute", "element.setAttribute", (a) => {
    const name = apply(strToLower, String(a[0]), []);
    const dangerous =
      apply(strSlice, name, [0, 2]) === "on" ||
      name === "href" ||
      name === "src" ||
      name === "srcdoc" ||
      name === "action" ||
      name === "formaction" ||
      name === "xlink:href";
    return dangerous ? a[1] : undefined;
  });
  for (const timer of ["setTimeout", "setInterval"]) {
    hookMethod(g, timer, timer + "(string)", (a) =>
      typeof a[0] === "string" ? a[0] : undefined,
    );
  }
  hookMethod(g, "open", "window.open", (a) =>
    a[0] == null ? undefined : String(a[0]),
  );
  // The Function constructor (eval itself is left alone: wrapping it would
  // turn direct eval calls into indirect ones and change their scope).
  try {
    const OriginalFunction = g.Function;
    const check = (args) => {
      try {
        if (args.length > 0) flow("Function()", String(args[args.length - 1]));
      } catch (_) {
        /* never break the page */
      }
    };
    g.Function = new Proxy(OriginalFunction, {
      apply(target, self, args) {
        check(args);
        return apply(target, self, args);
      },
      construct(target, args, newTarget) {
        check(args);
        return construct(
          target,
          args,
          newTarget === g.Function ? target : newTarget,
        );
      },
    });
  } catch (_) {
    /* not replaceable */
  }

  // Client-side redirects and navigations (location setters cannot be
  // hooked; the Navigation API reports their destination).
  try {
    if (g.navigation && typeof g.navigation.addEventListener === "function") {
      g.navigation.addEventListener("navigate", (e) => {
        try {
          flow("navigation", e.destination && e.destination.url, "redirect");
        } catch (_) {
          /* never break the page */
        }
      });
    }
  } catch (_) {
    /* no Navigation API */
  }

  // ---------------------------------------------------------------- postMessage
  try {
    g.addEventListener(
      "message",
      (e) => {
        try {
          const data = typeof e.data === "string" ? e.data : J(e.data);
          if (typeof data === "string" && data.length >= 2) {
            recentMessages.unshift(cut(data));
            recentMessages.length = Math.min(recentMessages.length, 20);
          }
        } catch (_) {
          /* unserializable data */
        }
      },
      true,
    );
  } catch (_) {
    /* no window */
  }
  const listeners = new WeakSet();
  // The prototype that gives the window its addEventListener (EventTarget
  // in browsers).
  const ET = (() => {
    try {
      for (let o = g.window || g; o; o = Object.getPrototypeOf(o)) {
        if (Object.prototype.hasOwnProperty.call(o, "addEventListener")) return o;
      }
    } catch (_) {
      /* no window */
    }
    return g.EventTarget && g.EventTarget.prototype;
  })();
  if (ET && typeof ET.addEventListener === "function") {
    const originalAdd = ET.addEventListener;
    const wrapper = {
      addEventListener(type, listener, options) {
        try {
          const onWindow =
            this === g || (g.window !== undefined && this === g.window);
          if (
            onWindow &&
            type === "message" &&
            listener &&
            !listeners.has(listener)
          ) {
            listeners.add(listener);
            const fn =
              typeof listener === "function" ? listener : listener.handleEvent;
            let code = "";
            try {
              code = apply(fnToString, fn, []);
            } catch (_) {
              code = "";
            }
            report({
              kind: "messageListener",
              sink: "window.addEventListener('message')",
              value: cut(code),
              source: null,
              sourceValue: null,
              matched: null,
              transformation: null,
              canary: false,
              checksOrigin: has(code, "origin"),
            });
          }
        } catch (_) {
          /* never break the page */
        }
        return apply(originalAdd, this, [type, listener, options]);
      },
    }.addEventListener;
    try {
      define(ET, "addEventListener", {
        value: wrapper,
        writable: true,
        configurable: true,
        enumerable: false,
      });
    } catch (_) {
      /* frozen */
    }
  }

  // ---------------------------------------------------------------- prototype pollution
  const baseline = new Set(getOwnNames(Object.prototype));
  const checkPollution = () => {
    try {
      const added = getOwnNames(Object.prototype).filter(
        (k) => !baseline.has(k),
      );
      for (const key of added) {
        baseline.add(key);
        let value = "";
        try {
          value = String(Object.prototype[key]);
        } catch (_) {
          value = "";
        }
        const t = trace(key) || trace(value);
        report({
          kind: "prototypePollution",
          sink: "Object.prototype." + key,
          value: cut(value),
          source: t ? t.source : null,
          sourceValue: t && t.sourceValue !== null ? cut(t.sourceValue) : null,
          matched: t ? t.matched : null,
          transformation: t ? t.transformation : null,
          canary: has(key, CANARY) || has(value, CANARY),
        });
      }
    } catch (_) {
      /* never break the page */
    }
  };

  // ---------------------------------------------------------------- DOM clobbering
  const documentBaseline = (() => {
    try {
      const fresh = g.document.implementation.createHTMLDocument("");
      const names = new Set();
      for (let o = fresh; o; o = Object.getPrototypeOf(o))
        for (const n of getOwnNames(o)) names.add(n);
      return names;
    } catch (_) {
      return new Set();
    }
  })();
  const checkClobbering = () => {
    try {
      const d = g.document;
      const els = d.querySelectorAll("[id],[name]");
      let reported = 0;
      for (const el of els) {
        if (reported >= 20) break;
        for (const n of [el.id, el.getAttribute("name")]) {
          if (!n || !documentBaseline.has(n)) continue;
          const v = d[n];
          if (
            v &&
            typeof v === "object" &&
            (v.nodeType === 1 || typeof v.length === "number")
          ) {
            reported += 1;
            const t = trace(n);
            report({
              kind: "clobbering",
              sink: "document." + n,
              value: cut(el.outerHTML.slice(0, 300)),
              source: t ? t.source : null,
              sourceValue:
                t && t.sourceValue !== null ? cut(t.sourceValue) : null,
              matched: t ? t.matched : null,
              transformation: t ? t.transformation : null,
              canary: false,
            });
          }
        }
      }
    } catch (_) {
      /* never break the page */
    }
  };

  const later = () => {
    checkPollution();
    checkClobbering();
  };
  try {
    g.addEventListener("DOMContentLoaded", later);
    g.addEventListener("load", () => {
      later();
      g.setTimeout(later, 2000);
    });
  } catch (_) {
    /* no window events */
  }
})();
