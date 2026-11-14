//! Robustness ("parser fuzzing", M16): every parser that reads untrusted
//! input gets thousands of corrupted variants of realistic messages and must
//! return — an error is fine, a panic or a hang is a bug.
//!
//! A deterministic mutator (no external fuzzing engine is available offline)
//! flips bits, truncates, duplicates, inserts boundary bytes and splices
//! known-tricky tokens. Seeds are fixed, so failures reproduce; set
//! `CHOSPROXY_ROBUSTNESS_ROUNDS` to run longer.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use std::time::{Duration, Instant};

struct Mutator(u64);

const TOKENS: &[&[u8]] = &[
    b"\r\n",
    b"\n",
    b"\r\n\r\n",
    b"0\r\n\r\n",
    b"ffffffffffffffff\r\n",
    b"Transfer-Encoding: chunked\r\n",
    b"Content-Length: 99999999999999999999\r\n",
    b"Content-Length: -1\r\n",
    b"Content-Encoding: gzip\r\n",
    b"Content-Encoding: br, deflate, gzip\r\n",
    b":",
    b";",
    b"=",
    b"%",
    b"%%",
    b"\x00",
    b"\xff\xfe",
    b"\xc3\x28",
    b"\xf0\x9f\x98",
    b"<",
    b"\"",
    b"'",
    b"\\",
    b"\xa7",
    b"\xc2\xa7",
    b"eyJ",
    b".",
    b"..",
    b"[",
    b"{",
];

impl Mutator {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    fn mutate(&mut self, seed: &[u8]) -> Vec<u8> {
        let mut out = seed.to_vec();
        for _ in 0..=self.below(4) {
            let at = self.below(out.len() + 1);
            match self.below(7) {
                0 if !out.is_empty() => {
                    let i = self.below(out.len());
                    out[i] ^= 1 << self.below(8);
                }
                1 => out.truncate(at),
                2 => {
                    let token = TOKENS[self.below(TOKENS.len())];
                    out.splice(at..at, token.iter().copied());
                }
                3 if !out.is_empty() => {
                    let end = (at + self.below(64)).min(out.len());
                    let piece = out[at.min(end)..end].to_vec();
                    out.splice(at..at, piece);
                }
                4 if !out.is_empty() => {
                    let end = (at + self.below(16)).min(out.len());
                    out.drain(at.min(end)..end);
                }
                5 => {
                    let byte = [0u8, 0x7f, 0x80, 0xff, b' ', b'\t'][self.below(6)];
                    out.insert(at, byte);
                }
                _ => {
                    let i = self.below(out.len().max(1));
                    if let Some(b) = out.get_mut(i) {
                        *b = (self.next() & 0xff) as u8;
                    }
                }
            }
        }
        out
    }
}

fn rounds() -> usize {
    std::env::var("CHOSPROXY_ROBUSTNESS_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1500)
}

/// Runs `check` on mutations of every seed, failing if any single input
/// takes too long.
fn fuzz(name: &str, seeds: &[&[u8]], mut check: impl FnMut(&[u8])) {
    let mut m = Mutator(0x2545_f491_4f6c_dd1d ^ name.len() as u64);
    let budget = Duration::from_secs(2);
    for seed in seeds {
        check(seed);
        for _ in 0..rounds() {
            let input = m.mutate(seed);
            let started = Instant::now();
            check(&input);
            assert!(
                started.elapsed() < budget,
                "{name}: one input took {:?}: {:?}",
                started.elapsed(),
                String::from_utf8_lossy(&input)
            );
        }
    }
}

const REQUEST: &[u8] = b"POST /login?next=/a&x=%41 HTTP/1.1\r\nHost: lab.test:8080\r\n\
    Cookie: sid=abc; theme=dark\r\nContent-Type: application/x-www-form-urlencoded\r\n\
    Content-Length: 21\r\n\r\nuser=bob&pass=secret1";
const CHUNKED: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nTransfer-Encoding: chunked\r\n\
    Set-Cookie: SESSIONID=abc123; Path=/; HttpOnly; SameSite=Lax\r\n\
    Set-Cookie: t=1; Max-Age=0; Expires=Wed, 21-Oct-2015 07:28:00 GMT; Domain=.lab.test\r\n\r\n\
    1a\r\n<html><a href=\"/x\">x</a>\r\n0\r\n\r\n";
const HTML: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 400\r\n\
    Access-Control-Allow-Origin: null\r\nServer: Apache/2.4.1\r\n\r\n\
    <html><base href=\"/b/\"><title>Index of /</title><script src=\"http://x/a.js\"></script>\
    <form action=\"http://lab.test/l\" method=get><input type=password name=p value=1>\
    <input type=hidden name=csrf value=\"abcdefgh12\"></form><meta http-equiv=refresh content=\"0;url=/n\">\
    <script>fetch('/api/v1?x=1'); var t=\"eyJhbGciOiJub25lIn0.eyJzdWIiOiIxIn0.\";</script>\
    You have an error in your SQL syntax 10.0.0.1 Traceback (most recent call last)</html>";

#[tokio::test]
async fn http_heads_bodies_and_framing() {
    use chosproxy_http::framing::{request_framing, response_framing};
    use chosproxy_http::head::{find_head_end, read_head};
    use chosproxy_http::{BodyReader, RequestHead, ResponseHead};
    use tokio::io::BufReader;

    let seeds: &[&[u8]] = &[REQUEST, CHUNKED, HTML];
    let mut inputs = Vec::new();
    fuzz("http-parse", seeds, |input| {
        let end = find_head_end(input).unwrap_or(input.len());
        if let Ok(h) = RequestHead::parse(input[..end].to_vec()) {
            let _ = request_framing(&h);
        }
        if let Ok(h) = ResponseHead::parse(input[..end].to_vec()) {
            let _ = h.keep_alive();
            let _ = response_framing("GET", &h);
        }
        if inputs.len() < 3000 {
            inputs.push(input.to_vec());
        }
    });
    // The async readers on a sample of the same inputs.
    for input in inputs {
        let mut reader = BufReader::new(&input[..]);
        let started = Instant::now();
        if let Ok(Some(raw)) = read_head(&mut reader, 64 * 1024).await
            && let Ok(head) = ResponseHead::parse(raw)
            && let Ok(framing) = response_framing("GET", &head)
        {
            let mut body = BodyReader::new(framing.kind);
            let mut total = 0usize;
            while let Ok(Some(chunk)) = body.next_chunk(&mut reader).await {
                total += chunk.len();
                assert!(total <= input.len() * 2, "a body grew beyond its input");
            }
        }
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

#[test]
fn content_decoding_and_text_editing() {
    use chosproxy_http::ResponseHead;
    use chosproxy_http::content::{body_text, dechunk, decode_content, split_message};
    use chosproxy_http::edit::{Eol, from_text, is_packed, to_text, unpack, update_content_length};
    use std::io::Write;

    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&HTML[..200]).unwrap();
    let gzipped = gz.finish().unwrap();
    let mut packed =
        b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    packed.extend_from_slice(format!("{:x}\r\n", gzipped.len()).as_bytes());
    packed.extend_from_slice(&gzipped);
    packed.extend_from_slice(b"\r\n0\r\n\r\n");

    fuzz("content", &[CHUNKED, &packed, REQUEST], |input| {
        let (head, body) = split_message(input);
        let _ = dechunk(body);
        for coding in ["gzip", "deflate", "br", "gzip, br", "x-unknown"] {
            let _ = decode_content(body, coding, 1024 * 1024);
        }
        if let Ok(h) = ResponseHead::parse(head.to_vec()) {
            let _ = body_text(&h, body, 1024 * 1024);
        }
        let _ = is_packed(input);
        let _ = unpack(input, 1024 * 1024);
        let _ = update_content_length(input, true);
        if let Ok(form) = to_text(input) {
            let _ = from_text(&form.text, Eol::CrLf, Eol::Lf);
        }
    });
}

#[test]
fn cookies_scope_and_websocket_close_frames() {
    use chosproxy_http::cookies::{
        SetCookie, default_path, domain_matches, path_matches, scope_of,
    };

    let seeds: &[&[u8]] = &[
        b"SESSIONID=abc; Path=/app; Domain=.Lab.Test; Secure; HttpOnly; SameSite=None; Max-Age=60",
        b"a=b; Expires=Wed, 21-Oct-2015 07:28:00 GMT",
        b"https://user@[::1]:8443/a/b?c=d#e",
        b"\x03\xe8closing",
    ];
    fuzz("cookies", seeds, |input| {
        let text = String::from_utf8_lossy(input);
        if let Some(c) = SetCookie::parse(&text, 1_700_000_000_000) {
            let _ = scope_of(&c, "www.lab.test", "/app/x");
            let _ = c.is_expired(0);
        }
        let _ = default_path(&text);
        let _ = path_matches(&text, "/app");
        let _ = domain_matches(&text, "lab.test", false);
        let _ = chosproxy_core::scope::UrlParts::parse(&text);
        let _ = chosproxy_websocket::parse_close(input);
    });
}

#[test]
fn passive_checks_crawler_discovery_and_sequencer_extraction() {
    use chosproxy_http::ResponseHead;
    use chosproxy_http::content::{body_text, split_message};
    use chosproxy_sequencer::TokenLocation;

    let page = url::Url::parse("https://lab.test/a/b").unwrap();
    let locations = [
        TokenLocation::Cookie {
            name: "SESSIONID".into(),
        },
        TokenLocation::Header {
            name: "Server".into(),
        },
        TokenLocation::Regex {
            pattern: r#"name=["']csrf["'][^>]*value=["']([^"']+)"#.into(),
        },
    ];
    fuzz("analysis", &[HTML, CHUNKED], |input| {
        let msg = chosproxy_scanner::Message::new(
            "https",
            "shop.example",
            443,
            "/login?token=abc&password=x",
            REQUEST,
            Some(input),
        );
        let _ = chosproxy_scanner::analyze(&msg, |_| true);
        let (head, body) = split_message(input);
        if let Ok(h) = ResponseHead::parse(head.to_vec()) {
            let text = body_text(&h, body, 1024 * 1024).unwrap_or_default();
            let _ = chosproxy_crawler::discover(&page, &h, &text, true);
        }
        let _ = chosproxy_sequencer::suggest(input);
        for l in &locations {
            let _ = l.extract(input);
        }
    });
}

#[test]
fn fuzzer_templates_sequencer_statistics_and_plugin_messages() {
    let seeds: &[&[u8]] = &[
        b"GET /?id=\xc2\xa71\xc2\xa7&q=\xc2\xa7x\xc2\xa7 HTTP/1.1\nCookie: a=b\n\nk=v&z=1",
        br#"{"type":"finding","exchangeId":3,"key":"k","kind":"K","title":"T","severity":"info"}"#,
        b"id = \"p\"\nname = \"P\"\nversion = \"1\"\napi = 1\ncommand = [\"x\"]\npermissions = [\"network.read\"]\n",
        b"0a1b2c3d4e5f\n0a1b2c3d4e60\nAAAA+/==\n123456\n",
        b"openapi: 3.0.0\ninfo: {title: t, version: '1'}\nservers: [{url: /v1}]\npaths:\n  /a/{id}:\n    post:\n      parameters: [{name: id, in: path, schema: {$ref: '#/components/schemas/X'}}]\n      requestBody: {content: {application/json: {schema: {$ref: '#/components/schemas/X'}}}}\ncomponents: {schemas: {X: {type: object, properties: {x: {$ref: '#/components/schemas/X'}}}}}\n",
        br#"{"info":{"name":"c","schema":"v2.1"},"variable":[{"key":"b","value":"http://x"}],"item":[{"name":"f","item":[{"name":"r","request":{"method":"POST","url":"{{b}}/p?q=1","body":{"mode":"formdata","formdata":[{"key":"a","value":"1"}]}}}]}]}"#,
        br#"<definitions xmlns="http://schemas.xmlsoap.org/wsdl/" xmlns:s="http://schemas.xmlsoap.org/wsdl/soap/" targetNamespace="urn:x"><binding name="B" type="P"><s:binding style="rpc"/><operation name="O"/></binding><service name="S"><port name="p" binding="B"><s:address location="http://x/s"/></port></service></definitions>"#,
    ];
    fuzz("text", seeds, |input| {
        let text = String::from_utf8_lossy(input);
        if let Ok(t) = chosproxy_fuzzer::Template::parse(&text) {
            let _ = t.render(&["x".into()]);
        }
        let points = chosproxy_scanner::insertion::insertion_points(
            &text,
            &chosproxy_scanner::PointKind::ALL,
        );
        let _ = chosproxy_scanner::insertion::mark(&text, &points, '§');
        let tokens: Vec<String> = text.lines().map(str::to_string).collect();
        let _ = chosproxy_sequencer::analyze(&tokens);
        let _ = chosproxy_apispec::parse(&text, Some("http://lab.test/"));
        let _ = chosproxy_plugin::PluginMessage::parse(&text);
        let _ = chosproxy_plugin::Manifest::parse(&text);
    });
}

#[test]
fn decoder_operations_on_garbage() {
    use chosproxy_decoder::{Operation, apply, smart_decode};
    let seeds: &[&[u8]] = &[
        b"SGVsbG8gd29ybGQ%3D%3D",
        b"eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln",
        b"\\u0041\\x42&lt;&#x43;%44+",
        b"{\"a\":[1,2,{\"b\":null}]}",
        b"\x1f\x8b\x08\x00\x00\x00\x00\x00",
    ];
    let mut n = 0usize;
    fuzz("decoder", seeds, |input| {
        n += 1;
        // Every operation on a share of the inputs keeps the run short.
        if n.is_multiple_of(7) {
            for op in Operation::ALL {
                let _ = apply(op, input);
            }
        }
        let _ = smart_decode(input);
    });
}

#[test]
fn oast_dns_questions_identifiers_and_api_pages() {
    use chosproxy_oast::dns::{self, ZoneAddresses};
    let id = chosproxy_oast::new_id().unwrap();
    let query = dns::build_query(0x4242, &format!("{id}.oast.example"), dns::TYPE_A);
    let page = format!(
        r#"{{"interactions":[{{"seq":1,"at":5,"protocol":"dns","remote":"1.2.3.4:5","ids":["{id}"],"summary":"A x","raw":"AAEC"}}],"latest":1,"zone":"oast.example"}}"#
    );
    let addresses = ZoneAddresses {
        ipv4: Some(std::net::Ipv4Addr::new(192, 0, 2, 1)),
        ipv6: None,
    };
    fuzz("oast", &[&query, page.as_bytes(), id.as_bytes()], |input| {
        if let Ok(q) = dns::parse_query(input) {
            let answer = dns::respond(&q, "oast.example", addresses);
            // Never an amplifier: the answer stays close to the question.
            assert!(
                answer.len() <= input.len() + 32,
                "answer {} for {}",
                answer.len(),
                input.len()
            );
        }
        let _ = chosproxy_oast::extract_ids(input);
        let _ = serde_json::from_slice::<chosproxy_oast::api::InteractionPage>(input);
    });
}

#[test]
fn dom_inspector_reports() {
    let seed = br#"{"kind":"flow","sink":"element.innerHTML","value":"<b>chosabc</b>","source":"location.hash","sourceValue":"chosabc","matched":"chosabc","transformation":"unchanged","canary":true,"frameUrl":"http://lab.test/dom/hash#chosabc","topFrame":true,"at":5,"stack":["    at f (http://lab.test/dom/hash:8:49)"]}"#;
    fuzz("dom", &[seed], |input| {
        if let Ok(report) = chosproxy_dom::DomReport::parse(&String::from_utf8_lossy(input)) {
            assert!(report.value.len() <= 2000 && report.stack.len() <= 20);
            let _ = report.location();
        }
    });
}
