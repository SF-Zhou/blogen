//! `blogen serve` – local preview server with optional watch/rebuild.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use notify::RecursiveMode;
use percent_encoding::percent_decode_str;

use crate::config::LoadedConfig;
use crate::error::{Error, Result};
use crate::site;

/// Options for the development server.
#[derive(Debug, Clone)]
pub struct ServeOptions {
    /// Address to bind.
    pub host: String,
    /// Port to bind.
    pub port: u16,
    /// Rebuild the site when source files change.
    pub watch: bool,
    /// Output directory override from the command line.
    pub output_override: Option<PathBuf>,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 9000,
            watch: true,
            output_override: None,
        }
    }
}

/// Build the site once and serve it until the process is terminated.
pub fn serve(loaded: &LoadedConfig, options: &ServeOptions) -> Result<()> {
    let output_dir = output_dir_for(loaded, options);
    site::build(loaded)?;
    let version = Arc::new(AtomicU64::new(1));

    if options.watch {
        spawn_watcher(loaded.clone(), options.clone(), version.clone())?;
    }

    let address = format!("{}:{}", options.host, options.port);
    let server = tiny_http::Server::http(&address).map_err(|err| {
        Error::Cli(format!(
            "could not start the preview server on {address}: {err} \
             (is the port already in use?)"
        ))
    })?;
    println!(
        "Serving `{}` at http://{address}/ (press Ctrl-C to stop)",
        output_dir.display()
    );
    if options.watch {
        println!("Watching for changes; the browser reloads automatically.");
    }

    loop {
        match server.recv() {
            Ok(request) => {
                let output_dir = output_dir.clone();
                let version = version.clone();
                let watch = options.watch;
                std::thread::spawn(move || {
                    let response = handle_request(
                        &request,
                        &output_dir,
                        version.load(Ordering::Relaxed),
                        watch,
                    );
                    if let Err(err) = request.respond(response) {
                        tracing::debug!(error = %err, "failed to send HTTP response");
                    }
                });
            }
            Err(err) => {
                tracing::warn!(error = %err, "failed to receive HTTP request");
            }
        }
    }
}

fn output_dir_for(loaded: &LoadedConfig, options: &ServeOptions) -> PathBuf {
    options.output_override.clone().map_or_else(
        || loaded.output_dir(),
        |path| {
            if path.is_absolute() {
                path
            } else {
                loaded.root.join(path)
            }
        },
    )
}

/// Watch content/theme/config and rebuild in a background thread.
fn spawn_watcher(
    loaded: LoadedConfig,
    options: ServeOptions,
    version: Arc<AtomicU64>,
) -> Result<()> {
    use notify::Watcher;

    let (sender, receiver) = std::sync::mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(
        move |event: std::result::Result<notify::Event, notify::Error>| {
            match event {
                // Rebuilds read the config file and posts; `notify` reports
                // plain reads as `Access` events, which must not trigger
                // another rebuild (this would loop forever).
                Ok(event) if matches!(event.kind, notify::EventKind::Access(_)) => {}
                Ok(event) => {
                    tracing::trace!(kind = ?event.kind, paths = ?event.paths, "watched change");
                    let _ = sender.send(());
                }
                Err(err) => tracing::warn!(error = %err, "file watcher error"),
            }
        },
    )
    .map_err(|err| Error::io("creating the file watcher", std::io::Error::other(err)))?;

    let mut watch_paths = vec![loaded.posts_dir(), loaded.path.clone()];
    if let Some(theme_path) = &loaded.config.theme.path {
        let theme_path = if theme_path.is_absolute() {
            theme_path.clone()
        } else {
            loaded.root.join(theme_path)
        };
        if theme_path.is_dir() {
            watch_paths.push(theme_path);
        }
    }
    for path in &watch_paths {
        if path.exists() {
            watcher
                .watch(path, RecursiveMode::Recursive)
                .map_err(|err| {
                    Error::io(
                        format!("watching `{}`", path.display()),
                        std::io::Error::other(err),
                    )
                })?;
        }
    }

    std::thread::spawn(move || {
        // Keep the watcher alive for the lifetime of the server thread.
        let _watcher = watcher;
        while receiver.recv().is_ok() {
            // Debounce: wait until no event has arrived for 300 ms so a
            // multi-event editor save only causes one rebuild.
            let quiet_period = Duration::from_millis(300);
            loop {
                match receiver.recv_timeout(quiet_period) {
                    Ok(()) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            let result = reload_and_build(&loaded, &options);
            match result {
                Ok(()) => {
                    version.fetch_add(1, Ordering::Relaxed);
                    tracing::info!("rebuilt site after change");
                }
                Err(err) => {
                    eprintln!("rebuild failed: {}", crate::error::format_error_chain(&err));
                }
            }
            // Discard events that arrived while building; they belong to the
            // change that was just handled.
            while receiver.try_recv().is_ok() {}
        }
    });
    Ok(())
}

fn reload_and_build(loaded: &LoadedConfig, options: &ServeOptions) -> Result<()> {
    let mut fresh = LoadedConfig::discover(Some(&loaded.path))?;
    if let Some(overrides) = &options.output_override {
        fresh.override_output(overrides);
    }
    site::build(&fresh)?;
    Ok(())
}

fn handle_request(
    request: &tiny_http::Request,
    output_dir: &Path,
    version: u64,
    watch: bool,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let url = request.url().to_string();
    let path = url.split(['?', '#']).next().unwrap_or("/");
    let decoded = percent_decode_str(path).decode_utf8_lossy().to_string();

    if decoded == "/__blogen/version" {
        return response_with_type(
            200,
            version.to_string().into_bytes(),
            "text/plain; charset=utf-8",
        );
    }

    let Some(relative) = safe_relative_path(&decoded) else {
        return response_with_type(404, b"Not found".to_vec(), "text/plain; charset=utf-8");
    };
    let mut file_path = output_dir.join(&relative);
    if file_path.is_dir() || (decoded.ends_with('/') && !file_path.is_file()) {
        file_path = file_path.join("index.html");
    }
    let Ok(mut bytes) = std::fs::read(&file_path) else {
        return response_with_type(404, b"Not found".to_vec(), "text/plain; charset=utf-8");
    };
    let extension = file_path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let content_type = content_type(&extension);
    if watch && (extension == "html" || extension == "htm") {
        bytes = inject_live_reload(bytes);
    }
    response_with_type(200, bytes, content_type)
}

fn response_with_type(
    status: u16,
    bytes: Vec<u8>,
    content_type: &str,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
        .expect("static header name and valid value");
    let cache = tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..])
        .expect("static header");
    tiny_http::Response::from_data(bytes)
        .with_status_code(status)
        .with_header(header)
        .with_header(cache)
}

/// Convert a decoded URL path into an output-relative path, rejecting traversal.
fn safe_relative_path(url_path: &str) -> Option<PathBuf> {
    let trimmed = url_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some(PathBuf::from("index.html"));
    }
    let mut path = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => path.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(path)
}

fn content_type(extension: &str) -> &'static str {
    match extension {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

const LIVE_RELOAD_SCRIPT: &[u8] = br#"<script>
(function () {
  var current = null;
  function poll() {
    fetch("/__blogen/version", { cache: "no-store" })
      .then(function (response) { return response.text(); })
      .then(function (version) {
        if (current === null) { current = version; }
        else if (current !== version) { window.location.reload(); }
      })
      .catch(function () {});
  }
  setInterval(poll, 800);
})();
</script>
"#;

fn inject_live_reload(bytes: Vec<u8>) -> Vec<u8> {
    let marker = b"</body>";
    let Some(position) = find_last(&bytes, marker) else {
        let mut out = bytes;
        out.extend_from_slice(LIVE_RELOAD_SCRIPT);
        return out;
    };
    let mut out = Vec::with_capacity(bytes.len() + LIVE_RELOAD_SCRIPT.len());
    out.extend_from_slice(&bytes[..position]);
    out.extend_from_slice(LIVE_RELOAD_SCRIPT);
    out.extend_from_slice(&bytes[position..]);
    out
}

fn find_last(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_paths() {
        assert_eq!(
            safe_relative_path("/").unwrap(),
            PathBuf::from("index.html")
        );
        assert_eq!(
            safe_relative_path("/a/b.css").unwrap(),
            PathBuf::from("a/b.css")
        );
        assert!(safe_relative_path("/../etc/passwd").is_none());
        assert!(safe_relative_path("/a/../../x").is_none());
    }

    #[test]
    fn content_types() {
        assert!(content_type("css").starts_with("text/css"));
        assert_eq!(content_type("bin"), "application/octet-stream");
    }

    #[test]
    fn injects_live_reload_before_body_end() {
        let html = b"<html><body>hi</body></html>".to_vec();
        let injected = inject_live_reload(html);
        let text = String::from_utf8(injected).unwrap();
        assert_eq!(text.matches("</body>").count(), 1);
        assert!(text.find("__blogen/version").unwrap() < text.find("</body>").unwrap());
    }

    #[test]
    fn injects_live_reload_without_body_tag() {
        let injected = inject_live_reload(b"fragment".to_vec());
        let text = String::from_utf8(injected).unwrap();
        assert!(text.contains("fragment"));
        assert!(text.contains("__blogen/version"));
    }
}
