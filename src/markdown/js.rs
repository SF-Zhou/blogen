//! Lazily-initialized JavaScript engines for KaTeX and Prism.
//!
//! Both libraries are vendored under `vendor/js/` and executed with the
//! `quick-js` engine so that the exact server-side output of the original
//! original pipeline (KaTeX 0.18.1 and Prism 1.30.0) can be reproduced without
//! an external JavaScript runtime.
//!
//! # Design
//!
//! Compiling the vendored bundles costs tens of milliseconds and the QuickJS
//! context is not `Send`, so each OS thread that renders markdown gets a
//! dedicated worker thread with a large stack that owns the engine
//! ([`with_engine`]). Batching keeps the number of Rust -> JavaScript
//! transitions at two per article (one for all math expressions, one for all
//! code blocks); the caller only pays a channel round-trip.
//!
//! The worker thread uses a larger-than-default stack because QuickJS' parser
//! recurses deeply while compiling the minified KaTeX bundle.
//!
//! # Thread safety
//!
//! [`MarkdownRenderer`] is `Sync`: it only stores plain options. The worker
//! channel lives in a `thread_local!`, so no engine state is ever shared
//! between threads.

use std::cell::RefCell;
use std::sync::mpsc::{self, Sender};
use std::thread;

use crate::error::{Error, Result};

/// Vendored official KaTeX 0.18.1 browser bundle (UMD).
const KATEX_JS: &str = include_str!("../../vendor/js/katex.min.js");
/// Vendored official Prism 1.30.0 core bundle (includes markup/css/clike/js).
const PRISM_JS: &str = include_str!("../../vendor/js/prism.js");

/// Prism language components the original pipeline loads, in dependency order.
///
/// `clike` ships with the core bundle; `c` (and `cpp`, `protobuf`) depend on
/// it, `bash` provides the `sh`/`shell` aliases and `yaml` the `yml` alias.
const PRISM_COMPONENTS: &[&str] = &[
    include_str!("../../vendor/js/prism-bash.min.js"),
    include_str!("../../vendor/js/prism-cmake.min.js"),
    include_str!("../../vendor/js/prism-c.min.js"),
    include_str!("../../vendor/js/prism-cpp.min.js"),
    include_str!("../../vendor/js/prism-json.min.js"),
    include_str!("../../vendor/js/prism-lua.min.js"),
    include_str!("../../vendor/js/prism-nasm.min.js"),
    include_str!("../../vendor/js/prism-protobuf.min.js"),
    include_str!("../../vendor/js/prism-python.min.js"),
    include_str!("../../vendor/js/prism-rust.min.js"),
    include_str!("../../vendor/js/prism-toml.min.js"),
    include_str!("../../vendor/js/prism-yaml.min.js"),
];

/// Stack size of the engine worker thread (QuickJS parser recursion).
const WORKER_STACK_SIZE: usize = 16 * 1024 * 1024;

/// The two engine hooks installed into a fresh QuickJS context.
const HELPERS: &str = r#"
// QuickJS has no browser globals; KaTeX/Prism only touch them from DOM code
// paths which renderToString()/highlight() never call, but a silent console
// keeps warnings (e.g. KaTeX "unknown symbol") from throwing.
var console = {
  log: function () {}, warn: function () {}, error: function () {},
  info: function () {}, debug: function () {}, trace: function () {}
};

function __blogen_katex(tex, display) {
  return katex.renderToString(tex, {
    displayMode: !!display,
    throwOnError: false,
    macros: undefined
  });
}

function __blogen_highlight(code, lang) {
  var grammar = Prism.languages[lang];
  if (!grammar) { return null; }
  try {
    return Prism.highlight(code, grammar);
  } catch (e) {
    return null;
  }
}

// Input/result hand-off for large batches.
var __blogen_input = [];
var __blogen_output = [];

function __blogen_katex_batch() {
  var input = JSON.parse(__blogen_input);
  var out = [];
  for (var i = 0; i < input.length; i++) {
    out.push(__blogen_katex(input[i][0], input[i][1]));
  }
  __blogen_output = out;
  return out.length;
}

function __blogen_highlight_batch() {
  var input = JSON.parse(__blogen_input);
  var out = [];
  for (var i = 0; i < input.length; i++) {
    out.push(__blogen_highlight(input[i][0], input[i][1]));
  }
  __blogen_output = out;
  return out.length;
}
"#;

/// One batch of work for the engine thread.
enum Request {
    /// Render `(tex, display_mode)` pairs to KaTeX HTML.
    Math(Vec<(String, bool)>, Sender<Result<Vec<String>>>),
    /// Highlight `(code, language)` pairs; `None` means "no grammar"/failure.
    Code(Vec<(String, String)>, Sender<Result<Vec<Option<String>>>>),
}

/// Handle to the per-thread engine worker.
struct Worker {
    sender: Sender<Request>,
}

// Lazily started engine worker for the current thread.
thread_local! {
    static WORKER: RefCell<Option<Worker>> = const { RefCell::new(None) };
}

impl Worker {
    fn new() -> Result<Self> {
        let (sender, receiver) = mpsc::channel::<Request>();
        let builder = thread::Builder::new()
            .name("blogen-js".to_string())
            .stack_size(WORKER_STACK_SIZE);
        builder
            .spawn(move || {
                let engine = match Engine::new() {
                    Ok(engine) => engine,
                    Err(err) => {
                        // Answer every request with the initialization error.
                        while let Ok(request) = receiver.recv() {
                            fail_request(request, &err);
                        }
                        return;
                    }
                };
                while let Ok(request) = receiver.recv() {
                    run_request(&engine, request);
                }
            })
            .map_err(|err| {
                Error::Markdown(format!("failed to start JavaScript engine thread: {err}"))
            })?;
        Ok(Self { sender })
    }

    fn send(&self, request: Request) -> Result<()> {
        self.sender
            .send(request)
            .map_err(|_| Error::Markdown("JavaScript engine thread stopped".to_string()))
    }
}

/// Complete a request with a shared initialization error.
fn fail_request(request: Request, err: &Error) {
    let message = err.to_string();
    match request {
        Request::Math(_, reply) => {
            let _ = reply.send(Err(Error::Markdown(message)));
        }
        Request::Code(_, reply) => {
            let _ = reply.send(Err(Error::Markdown(message)));
        }
    }
}

/// Execute one request on the engine thread.
fn run_request(engine: &Engine, request: Request) {
    match request {
        Request::Math(items, reply) => {
            let _ = reply.send(engine.render_math(&items));
        }
        Request::Code(items, reply) => {
            let _ = reply.send(engine.highlight_code(&items));
        }
    }
}

/// Run `f` with the thread-local worker, creating it on first use.
fn with_worker<T>(f: impl FnOnce(&Worker) -> Result<T>) -> Result<T> {
    WORKER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Worker::new()?);
        }
        let worker = slot
            .as_ref()
            .ok_or_else(|| Error::Markdown("JavaScript engine worker unavailable".to_string()))?;
        f(worker)
    })
}

/// A QuickJS context with KaTeX and Prism loaded.
struct Engine {
    context: quick_js::Context,
}

impl Engine {
    fn new() -> Result<Self> {
        let context = quick_js::Context::new()
            .map_err(|err| Error::Markdown(format!("failed to create JavaScript engine: {err}")))?;
        let engine = Self { context };
        engine.eval(KATEX_JS, "katex.min.js")?;
        engine.eval(PRISM_JS, "prism.js")?;
        for component in PRISM_COMPONENTS {
            engine.eval(component, "prism component")?;
        }
        engine.eval(HELPERS, "blogen helpers")?;
        Ok(engine)
    }

    fn eval(&self, source: &str, what: &str) -> Result<()> {
        // Append a primitive completion value: `quick_js` converts the value
        // of the final expression into a Rust `JsValue`, and the grammar
        // objects assigned by the Prism components are large nested objects
        // (converting them recursively overflows the stack).
        let source = format!("{source}\n;0");
        self.context.eval(&source).map(|_| ()).map_err(|err| {
            Error::Markdown(format!(
                "failed to evaluate vendored JavaScript `{what}`: {err}"
            ))
        })
    }

    /// Run a batch function and return the JSON-encoded result array.
    fn run_batch(&self, call: &str, what: &str) -> Result<String> {
        let value = self
            .context
            .eval(call)
            .map_err(|err| Error::Markdown(format!("{what} failed: {err}")))?;
        value
            .into_string()
            .ok_or_else(|| Error::Markdown(format!("{what}: JavaScript did not return a string")))
    }

    fn render_math(&self, items: &[(String, bool)]) -> Result<Vec<String>> {
        let payload = serde_json::to_string(items)
            .map_err(|err| Error::Markdown(format!("failed to serialize math batch: {err}")))?;
        self.context
            .set_global("__blogen_input", payload)
            .map_err(|err| Error::Markdown(format!("failed to pass math to JavaScript: {err}")))?;
        let raw = self.run_batch(
            "__blogen_katex_batch(); JSON.stringify(__blogen_output)",
            "KaTeX rendering",
        )?;
        serde_json::from_str(&raw)
            .map_err(|err| Error::Markdown(format!("failed to decode KaTeX output: {err}")))
    }

    fn highlight_code(&self, items: &[(String, String)]) -> Result<Vec<Option<String>>> {
        let payload = serde_json::to_string(items)
            .map_err(|err| Error::Markdown(format!("failed to serialize code batch: {err}")))?;
        self.context
            .set_global("__blogen_input", payload)
            .map_err(|err| {
                Error::Markdown(format!("failed to pass code block to JavaScript: {err}"))
            })?;
        let raw = self.run_batch(
            "__blogen_highlight_batch(); JSON.stringify(__blogen_output)",
            "Prism highlighting",
        )?;
        serde_json::from_str(&raw)
            .map_err(|err| Error::Markdown(format!("failed to decode Prism output: {err}")))
    }
}

/// Render a batch of TeX expressions with KaTeX.
///
/// `items` holds `(tex, display_mode)` pairs; the returned vector has the same
/// length and order.
pub fn render_math(items: &[(String, bool)]) -> Result<Vec<String>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let items = items.to_vec();
    with_worker(|worker| {
        let (reply, response) = mpsc::channel();
        worker.send(Request::Math(items, reply))?;
        response
            .recv()
            .map_err(|_| Error::Markdown("JavaScript engine thread stopped".to_string()))?
    })
}

/// Highlight a batch of `(code, language)` pairs with Prism.
///
/// The returned string is `None` when Prism has no grammar for the language or
/// highlighting throws, exactly like the original pipeline's fallback path.
pub fn highlight_code(items: &[(String, String)]) -> Result<Vec<Option<String>>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let items = items.to_vec();
    with_worker(|worker| {
        let (reply, response) = mpsc::channel();
        worker.send(Request::Code(items, reply))?;
        response
            .recv()
            .map_err(|_| Error::Markdown("JavaScript engine thread stopped".to_string()))?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn katex_engine_renders_server_side() {
        let items = vec![("e^{i\\pi}+1=0".to_string(), true)];
        let out = render_math(&items).expect("katex batch");
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("katex-display"), "{}", out[0]);
        assert!(out[0].contains("mord"), "{}", out[0]);
    }

    #[test]
    fn prism_engine_highlights_known_and_reports_unknown() {
        let items = vec![
            ("let x = 1;".to_string(), "rust".to_string()),
            ("x".to_string(), "sql".to_string()),
        ];
        let out = highlight_code(&items).expect("prism batch");
        assert_eq!(out.len(), 2);
        assert!(out[0].as_deref().unwrap().contains("token keyword"));
        assert!(out[1].is_none());
    }
}
