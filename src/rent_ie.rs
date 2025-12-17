use scraper::{ElementRef, Html, Selector};
use std::sync::LazyLock;

#[derive(Debug)]
pub(crate) struct Selectors {
    pub(crate) result: Selector,
    pub(crate) img: Selector,
    pub(crate) link_img: Selector,
    pub(crate) date: Selector,
    pub(crate) price: Selector,
    pub(crate) description: Selector,
}

#[allow(clippy::unwrap_used)]
pub(crate) static SELECTORS: LazyLock<Selectors> = LazyLock::new(|| Selectors {
    result: Selector::parse("div.search_result").unwrap(),
    img: Selector::parse(".sresult_thumb").unwrap(),
    link_img: Selector::parse(".search_more_info_image").unwrap(),
    date: Selector::parse("span.sresult_available_from").unwrap(),
    price: Selector::parse(".sresult_description > h4").unwrap(),
    description: Selector::parse("div:has(> .sresult_available_from)").unwrap(),
});

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

impl<'a> From<ElementRef<'a>> for RentListing {
    fn from(entry: ElementRef<'a>) -> Self {
        let img = entry.select(&SELECTORS.img).next();

        let img_url = img
            .and_then(|i| i.value().attr("src"))
            .map(ToString::to_string)
            .unwrap_or_default();

        let address = img
            .and_then(|i| i.value().attr("alt"))
            .map(ToString::to_string)
            .unwrap_or_default();

        let link = entry
            .select(&SELECTORS.link_img)
            .next()
            .and_then(|el| el.parent())
            .and_then(|p| p.value().as_element())
            .and_then(|el| el.attr("href"))
            .map(ToString::to_string)
            .unwrap_or_default();

        let id = link
            .split('/')
            .skip(4)
            .take(2)
            .collect::<Vec<_>>()
            .join("/");

        let date = entry
            .select(&SELECTORS.date)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let price = entry
            .select(&SELECTORS.price)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let description = entry
            .select(&SELECTORS.description)
            .next()
            .map(|el| {
                el.text()
                    .collect::<String>()
                    .trim()
                    .replace(&date, "")
                    .trim()
                    .to_string()
            })
            .unwrap_or_default();

        Self {
            id,
            link,
            img_url,
            address,
            price,
            date,
            description,
        }
    }
}

pub(crate) fn extract_listings_from_html(html: &str) -> Vec<RentListing> {
    Html::parse_document(html)
        .select(&SELECTORS.result)
        .map(RentListing::from)
        .collect()
}
