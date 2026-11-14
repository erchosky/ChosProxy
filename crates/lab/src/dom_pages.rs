//! Deliberately vulnerable client-side pages for the DOM Inspector
//! (`/dom/…`). Each has one textbook flaw and is only served by the local
//! lab server; nothing here is reachable unless you run the lab.

/// Index of the DOM demo pages.
pub const INDEX: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>DOM lab</title></head>
<body>
<h1>DOM Inspector lab</h1>
<p>Intentionally vulnerable pages. Open them in the lab browser with the DOM
Inspector on and put the canary where the page reads its input.</p>
<ul>
  <li><a href="/dom/hash#guest">/dom/hash</a>: the fragment goes into <code>innerHTML</code></li>
  <li><a href="/dom/search?q=shoes">/dom/search</a>: the <code>q</code> parameter goes into <code>document.write</code></li>
  <li><a href="/dom/redirect">/dom/redirect</a>: <code>?next=</code> decides where the page navigates</li>
  <li><a href="/dom/message">/dom/message</a>: a <code>message</code> listener without an origin check writes HTML</li>
  <li><a href="/dom/pollution">/dom/pollution</a>: query keys are merged into an object naively</li>
  <li><a href="/dom/clobber">/dom/clobber</a>: an element named <code>referrer</code> shadows <code>document.referrer</code></li>
</ul>
</body></html>
"#;

/// Fragment → innerHTML.
pub const HASH: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Welcome</title></head>
<body>
<h1 id="greeting"></h1>
<script>
  // Vulnerable on purpose: the fragment is written as HTML.
  var name = decodeURIComponent(location.hash.slice(1) || "guest");
  document.getElementById("greeting").innerHTML = "Welcome, " + name;
</script>
</body></html>
"#;

/// Query parameter → document.write.
pub const SEARCH: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Search</title></head>
<body>
<h1>Search</h1>
<script>
  // Vulnerable on purpose: the parameter is written into the document.
  var q = new URLSearchParams(location.search).get("q") || "";
  document.write("<p>Results for " + q + "</p>");
</script>
</body></html>
"#;

/// Query parameter → navigation.
pub const REDIRECT: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Redirect</title></head>
<body>
<p>Continue to <a id="go" href="/dom/">the lab</a>.</p>
<script>
  // Vulnerable on purpose: the destination comes from the URL.
  var next = new URLSearchParams(location.search).get("next");
  if (next) {
    document.getElementById("go").href = next;
    setTimeout(function () { location.href = next; }, 1500);
  }
</script>
</body></html>
"#;

/// postMessage without an origin check → innerHTML.
pub const MESSAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Messages</title></head>
<body>
<div id="inbox">No message yet.</div>
<button id="send">Post the fragment to this window</button>
<script>
  // Vulnerable on purpose: any origin may post HTML here.
  window.addEventListener("message", function (event) {
    document.getElementById("inbox").innerHTML = event.data;
  });
  document.getElementById("send").onclick = function () {
    window.postMessage(decodeURIComponent(location.hash.slice(1)), "*");
  };
</script>
</body></html>
"#;

/// Naive deep merge of query keys → prototype pollution.
pub const POLLUTION: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Settings</title></head>
<body>
<p>Settings are read from the query string, e.g. <code>?theme[color]=red</code>.</p>
<script>
  // Vulnerable on purpose: keys like __proto__[x] reach Object.prototype.
  var settings = {};
  new URLSearchParams(location.search).forEach(function (value, key) {
    var parts = key.replace(/\]/g, "").split("[");
    var target = settings;
    for (var i = 0; i < parts.length - 1; i++) {
      target[parts[i]] = target[parts[i]] || {};
      target = target[parts[i]];
    }
    target[parts[parts.length - 1]] = value;
  });
</script>
</body></html>
"#;

/// An element shadows a document property.
pub const CLOBBER: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Clobbering</title></head>
<body>
<img name="referrer" src="data:,">
<p>The image above is named <code>referrer</code>, so <code>document.referrer</code> is now an element.</p>
</body></html>
"#;
