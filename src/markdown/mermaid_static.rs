//! Build-time (static) Mermaid rendering.
//!
//! Mermaid diagrams are rendered to SVG while the site is built, using the
//! pure-Rust [`merman`](https://crates.io/crates/merman) renderer. No browser,
//! JavaScript runtime or Node.js is involved, and the generated site contains
//! no Mermaid JavaScript.
//!
//! For each diagram two SVGs are produced:
//!
//! * the light variant using Mermaid's default theme;
//! * the dark variant using Mermaid's dark theme.
//!
//! They are wrapped in the same markup the previous generator produced:
//!
//! ```html
//! <figure class="mermaid" data-mermaid-digest="…">
//!   <div class="mermaid-light"><svg …>…</svg></div>
//!   <div class="mermaid-dark"><svg …>…</svg></div>
//! </figure>
//! ```
//!
//! The `data-mermaid-digest` attribute is the MD5 of the diagram source; it is
//! used by the parity tests and makes generated pages easy to diff.
//!
//! The visual output is close to, but not byte-identical with, the previous
//! browser-based renderer (Mermaid version, theme palette and text measuring
//! differ slightly). This is an accepted trade-off for fully static output.
//!
//! Renderers are expensive to initialize and are not `Sync`, so a lazily
//! created pair lives in a thread local; markdown articles are rendered in
//! parallel by rayon, so each worker thread gets its own renderers.

use std::cell::RefCell;

use merman::{
    MermaidEngine, OperationControl, RenderOutput, RenderRequest, Renderer,
    SourcePresentationConfig, SvgRequest,
};

use crate::error::{Error, Result};

thread_local! {
    static RENDERERS: RefCell<Option<Renderers>> = const { RefCell::new(None) };
}

/// One renderer per theme variant.
struct Renderers {
    light: Renderer,
    dark: Renderer,
}

impl Renderers {
    fn new() -> Self {
        Self {
            light: theme_renderer("default"),
            dark: theme_renderer("dark"),
        }
    }
}

/// Build a renderer configured with a Mermaid theme name.
fn theme_renderer(theme: &str) -> Renderer {
    let config = SourcePresentationConfig::from_value(serde_json::json!({ "theme": theme }));
    Renderer::new().with_presentation_engine(MermaidEngine::new().with_site_config(config))
}

/// Render one Mermaid diagram to the static figure markup.
///
/// `diagram_index` is the zero-based index of the diagram inside the current
/// document; together with the source digest it produces stable, unique SVG
/// ids (`mermaid-<digest>-<index>-light` / `-dark`), matching the id scheme of
/// the previous generator.
pub(crate) fn render(diagram: &str, diagram_index: usize) -> Result<String> {
    let source = diagram.strip_suffix('\n').unwrap_or(diagram);
    let digest = crate::hashing::md5_hex(source.as_bytes());
    let base_id = format!("mermaid-{}-{diagram_index}", &digest[..8]);

    RENDERERS.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Renderers::new());
        }
        let renderers = slot.as_ref().expect("renderers were initialized above");
        let light = render_one(&renderers.light, source, &format!("{base_id}-light"))?;
        let dark = render_one(&renderers.dark, source, &format!("{base_id}-dark"))?;
        Ok(format!(
            "<figure class=\"mermaid\" data-mermaid-digest=\"{digest}\">\
             <div class=\"mermaid-light\">{light}</div>\
             <div class=\"mermaid-dark\">{dark}</div></figure>"
        ))
    })
}

/// Render one diagram variant to an SVG string.
fn render_one(renderer: &Renderer, source: &str, diagram_id: &str) -> Result<String> {
    let request = SvgRequest {
        options: merman::svg::SvgRenderOptions {
            diagram_id: Some(diagram_id.to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    let request = RenderRequest::svg(source, OperationControl::new(), request);
    match renderer.render(request) {
        Ok(RenderOutput::Svg(Some(svg))) => Ok(svg.svg().to_string()),
        Ok(RenderOutput::Svg(None)) => Err(Error::Markdown(format!(
            "Mermaid diagram was not recognised by the renderer ({diagram_id})"
        ))),
        Ok(_) => Err(Error::Markdown(format!(
            "Mermaid renderer returned an unexpected output ({diagram_id})"
        ))),
        Err(error) => Err(Error::Markdown(format!(
            "failed to render Mermaid diagram {diagram_id} ({error}): {}",
            first_line(source)
        ))),
    }
}

/// First line of a diagram, used to make render errors identifiable.
fn first_line(source: &str) -> &str {
    source.lines().next().unwrap_or(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_light_and_dark_svg_figures() {
        let html = render("graph TD\n  A[Start] --> B[Done]\n", 0).expect("render");
        assert!(html.starts_with("<figure class=\"mermaid\" data-mermaid-digest=\""));
        assert!(html.contains("<div class=\"mermaid-light\"><svg id=\"mermaid-"));
        assert!(html.contains("-0-light\""), "light svg id: {html}");
        assert!(html.contains("<div class=\"mermaid-dark\"><svg id=\"mermaid-"));
        assert!(html.contains("-0-dark\""), "dark svg id: {html}");
        assert!(html.ends_with("</figure>"));
        assert_eq!(html.matches("<svg").count(), 2);
        assert!(!html.contains("mermaid-source"));
    }

    #[test]
    fn cjk_sequence_diagram_is_supported() {
        let source = "sequenceDiagram\n    autonumber\n    participant L as 本地\n    participant R as 远端\n    L->>+R: 发起 RPC 请求\n    Note over R: 准备好 Buffer\n    R-->>-L: 返回\n";
        let html = render(source, 3).expect("render sequence diagram");
        assert!(html.contains("本地"));
        assert!(html.contains("<div class=\"mermaid-dark\"><svg id=\"mermaid-"));
    }

    #[test]
    fn diagram_ids_are_stable_and_unique() {
        let source = "graph LR\n  A --> B\n";
        let first = render(source, 1).expect("render");
        let second = render(source, 2).expect("render");
        // `render` strips the fence's trailing newline before hashing.
        let digest =
            crate::hashing::md5_hex(source.strip_suffix('\n').unwrap_or(source).as_bytes());
        assert!(first.contains(&format!("mermaid-{}-1-light", &digest[..8])));
        assert!(second.contains(&format!("mermaid-{}-2-light", &digest[..8])));
        assert!(first.contains(&format!("data-mermaid-digest=\"{digest}\"")));
    }

    #[test]
    fn unsupported_source_reports_an_error() {
        let error = render("definitely not mermaid", 0).expect_err("must fail");
        assert!(matches!(error, Error::Markdown(_)));
        assert!(error.to_string().contains("definitely not mermaid"));
    }

    #[test]
    fn md5_hex_matches_known_value() {
        // `data-mermaid-digest` is covered by the fixture parity tests; this
        // pins the digest algorithm to the established MD5 of the old pipeline.
        assert_eq!(
            crate::hashing::md5_hex(b""),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
    }
}
