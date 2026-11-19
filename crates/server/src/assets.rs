use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::app::AppState;

#[derive(RustEmbed)]
#[folder = "../../clients/web/dist/"]
struct Assets;

fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 按配置的自定义标题重写内嵌 `index.html` 的 `<title>` 标签。
///
/// 只替换第一处 `<title>gateterm</title>`,保持侧边栏 `<span id="logo">gateterm</span>` 等不变。
pub fn render_index_html(html: &str, web_title: Option<&str>) -> String {
    let title = crate::config::format_web_title(web_title);
    let escaped = escape_html(&title);
    html.replacen("<title>gateterm</title>", &format!("<title>{escaped}</title>"), 1)
}

pub async fn static_handler(State(state): State<AppState>, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let (name, content) = match Assets::get(path) {
        Some(content) => (path, content),
        // SPA 回退:未命中一律回 index.html,mime 必须按实际返回的文件算
        None => match Assets::get("index.html") {
            Some(content) => ("index.html", content),
            None => return (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    };
    if name == "index.html" {
        let html = String::from_utf8_lossy(&content.data);
        let rendered = render_index_html(&html, state.web_title.get().as_deref());
        return (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            rendered,
        )
            .into_response();
    }
    let mime = mime_guess::from_path(name).first_or_octet_stream();
    ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_index_html_has_expected_title_and_logo_markers() {
        let asset = Assets::get("index.html").expect("embedded index.html must exist");
        let html = std::str::from_utf8(&asset.data).expect("index.html must be utf-8");
        assert!(html.contains("<title>gateterm</title>"));
        assert!(html.contains(r#"<span id="logo">gateterm</span>"#));
    }

    #[test]
    fn render_index_html_keeps_default_when_unset() {
        let html = "<head><title>gateterm</title></head><body><span id=\"logo\">gateterm</span></body>";
        assert_eq!(render_index_html(html, None), html);
        assert_eq!(render_index_html(html, Some("   ")), html);
    }

    #[test]
    fn render_index_html_replaces_only_title_and_escapes_html() {
        let html = "<head><title>gateterm</title></head><body><span id=\"logo\">gateterm</span></body>";
        let custom = render_index_html(html, Some("prod-01"));
        assert!(custom.contains("<title>prod-01 - gateterm</title>"));
        assert!(custom.contains(r#"<span id="logo">gateterm</span>"#));

        let injected = render_index_html(html, Some(r#"</title><script>alert(1)</script> & "x" 'y'"#));
        assert!(injected.contains(
            "<title>&lt;/title&gt;&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;x&quot; &#39;y&#39; - gateterm</title>"
        ));
        assert!(!injected.contains("<script>"));
    }
}
