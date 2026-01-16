use tl::{HTMLTag, Node, NodeHandle, ParserOptions, VDom};

// Note, descdendent selectors aren't implemented in tl: https://github.com/y21/tl/issues/22
const RESULT_SELECTOR: &str = "div.search_result";
const IMG_SELECTOR: &str = ".sresult_thumb";
const LINK_PARENT_SELECTOR: &str = "h2";
const LINK_SELECTOR: &str = "a";
const DATE_SELECTOR: &str = "span.sresult_available_from";
const PRICE_SELECTOR: &str = "h4";
const DESCRIPTION_GRANDPARENT_SELECTOR: &str = ".sresult_description";
const DESCRIPTION_PARENT_SELECTOR: &str = "div";

#[derive(Debug, Clone)]
pub(crate) struct RentListing {
    pub(crate) id: String,
    pub(crate) link: String,
    pub(crate) img_url: String,
    pub(crate) address: String,
    pub(crate) price: String,
    pub(crate) date: String,
    pub(crate) description: String,
}

#[inline]
fn get_tag_attr_value(tag: &HTMLTag, name: &str) -> Option<String> {
    tag.attributes()
        .get(name)
        .flatten()
        .map(|v| v.as_utf8_str().to_string())
}

#[inline]
fn find_description_string(node_handle: NodeHandle, dom: &VDom) -> Option<String> {
    let node = node_handle.get(dom.parser())?;

    let raw_bytes = match node {
        Node::Raw(raw_node) => raw_node.clone(),
        _ => return None,
    };

    let byte_string = raw_bytes.as_utf8_str().trim().to_string();

    if byte_string.is_empty() {
        return None;
    }

    Some(byte_string)
}

impl RentListing {
    fn from_node(node: &Node, dom: &VDom) -> Result<Self, String> {
        let tag = node.as_tag().ok_or("Failed to get tag from node")?;

        let (img_url, address) = {
            let img = tag
                .query_selector(dom.parser(), IMG_SELECTOR)
                .and_then(|mut q| q.next())
                .and_then(|n| n.get(dom.parser()))
                .and_then(Node::as_tag);

            let img_url = img
                .and_then(|t| get_tag_attr_value(t, "data-original"))
                .unwrap_or_default();
            let address = img
                .and_then(|t| get_tag_attr_value(t, "alt"))
                .unwrap_or_default();

            (img_url, address)
        };

        let link = {
            let link_parent = tag
                .query_selector(dom.parser(), LINK_PARENT_SELECTOR)
                .and_then(|mut q| q.next())
                .and_then(|n| n.get(dom.parser()))
                .and_then(Node::as_tag);

            link_parent
                .and_then(|n| n.query_selector(dom.parser(), LINK_SELECTOR))
                .and_then(|mut q| q.next())
                .and_then(|n| n.get(dom.parser()))
                .and_then(Node::as_tag)
                .and_then(|t| get_tag_attr_value(t, "href"))
                .unwrap_or_default()
        };

        let id = link
            .split('/')
            .skip(4)
            .take(2)
            .collect::<Vec<_>>()
            .join("/");

        let date = tag
            .query_selector(dom.parser(), DATE_SELECTOR)
            .and_then(|mut q| q.next())
            .and_then(|n| n.get(dom.parser()))
            .and_then(|n| n.as_tag())
            .map(|t| t.inner_text(dom.parser()).trim().to_string())
            .unwrap_or_default();

        let price = tag
            .query_selector(dom.parser(), PRICE_SELECTOR)
            .and_then(|mut q| q.next())
            .and_then(|n| n.get(dom.parser()))
            .and_then(|n| n.as_tag())
            .map(|t| t.inner_text(dom.parser()).trim().replace("&euro;", "€"))
            .unwrap_or_default();

        let description = {
            let description_grandparent = tag
                .query_selector(dom.parser(), DESCRIPTION_GRANDPARENT_SELECTOR)
                .and_then(|mut q| q.next())
                .and_then(|n| n.get(dom.parser()))
                .and_then(Node::as_tag);

            let description_parent = description_grandparent
                .and_then(|n| n.query_selector(dom.parser(), DESCRIPTION_PARENT_SELECTOR))
                .and_then(|mut q| q.next())
                .and_then(|n| n.get(dom.parser()));

            description_parent
                .and_then(Node::children)
                .and_then(|n| {
                    n.top()
                        .iter()
                        .find_map(|n| find_description_string(*n, dom))
                })
                .map(|n| n.replace(&date, "").trim().to_string())
                .unwrap_or_default()
        };

        let listing = Self {
            id,
            link,
            img_url,
            address,
            price,
            date,
            description,
        };

        Ok(listing)
    }
}

pub(crate) fn extract_listings_from_html(html: &str) -> Result<Vec<RentListing>, String> {
    let dom = tl::parse(html, ParserOptions::default())
        .map_err(|e| format!("Failed to parse HTML: {e}"))?;
    let parser = dom.parser();

    dom.query_selector(RESULT_SELECTOR)
        .ok_or("Result selector query failed")?
        .filter_map(|n| n.get(parser))
        .map(|node| RentListing::from_node(node, &dom))
        .collect()
}
