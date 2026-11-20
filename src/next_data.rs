//! Helpers shared by the Next.js sites (rent.ie and property.ie).

use nanoserde::DeJson;
use tl::{Node, ParserOptions};

const NEXT_DATA_SELECTOR: &str = "script#__NEXT_DATA__";

/// `pageProps.paging` of a `_next/data` search response.
#[derive(Debug, DeJson)]
// nanoserde-derive panics on `pub(crate)` visibility, so this is `pub`.
pub struct Paging {
    #[nserde(rename = "currentPage")]
    current_page: u32,
    #[nserde(rename = "totalPages")]
    total_pages: u32,
}

impl Paging {
    pub(crate) fn next_page(&self) -> Option<u32> {
        (self.current_page < self.total_pages).then_some(self.current_page + 1)
    }
}

/// Returns the JSON text of the page's `<script id="__NEXT_DATA__">`.
pub(crate) fn json_from_html(html: &str) -> Result<String, String> {
    let dom = tl::parse(html, ParserOptions::default())
        .map_err(|error| format!("Failed to parse HTML: {error}"))?;

    let script = dom
        .query_selector(NEXT_DATA_SELECTOR)
        .and_then(|mut selector| selector.next())
        .and_then(|node_handle| node_handle.get(dom.parser()))
        .and_then(Node::as_tag)
        .ok_or("Failed to find __NEXT_DATA__ script in HTML")?;

    Ok(script.inner_text(dom.parser()).to_string())
}

/// Resolves a Next.js page pattern such as `/search/[[...rest]]` into the
/// `_next/data` route (`search/a/b`), defaulting to `search`.
pub(crate) fn data_route(page: &str, rest: &[String]) -> String {
    let route = page
        .replace("[[...rest]]", &rest.join("/"))
        .trim_matches('/')
        .to_string();

    if route.is_empty() {
        "search".to_string()
    } else {
        route
    }
}

/// Returns the slug from a `/<prefix>/<slug>/<id>` path, or `""`.
pub(crate) fn slug_from_path(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .nth(1)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{Paging, data_route, slug_from_path};

    #[test]
    fn resolves_data_routes() {
        let rest = vec!["a".to_string(), "b".to_string()];
        assert_eq!(data_route("/search/[[...rest]]", &rest), "search/a/b");
        assert_eq!(data_route("/[[...rest]]", &[]), "search");
    }

    #[test]
    fn extracts_slugs_and_next_pages() {
        assert_eq!(slug_from_path("/share/some-slug/123/"), "some-slug");
        assert_eq!(slug_from_path("123"), "");
        let paging = Paging {
            current_page: 1,
            total_pages: 2,
        };
        assert_eq!(paging.next_page(), Some(2));
    }
}
