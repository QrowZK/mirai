use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;

/// The kind of resource a network request is fetching, mirroring the subset of
/// fetch destinations the filter engine distinguishes. The browser maps
/// Servo's `Destination` to this type so this crate stays Servo-free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    Document,
    SubDocument,
    Script,
    Style,
    Image,
    Font,
    Media,
    Xhr,
    WebSocket,
    Other,
}

impl RequestKind {
    fn as_adblock_type(self) -> &'static str {
        match self {
            RequestKind::Document => "main_frame",
            RequestKind::SubDocument => "sub_frame",
            RequestKind::Script => "script",
            RequestKind::Style => "stylesheet",
            RequestKind::Image => "image",
            RequestKind::Font => "font",
            RequestKind::Media => "media",
            RequestKind::Xhr => "xhr",
            RequestKind::WebSocket => "websocket",
            RequestKind::Other => "other",
        }
    }
}

/// A filter-list based request blocker backed by Brave's `adblock` engine.
pub struct Blocker {
    engine: Engine,
}

impl Blocker {
    /// Build a blocker from raw filter-list contents (one list per string,
    /// standard ABP/EasyList syntax).
    pub fn from_filter_lists<'a>(lists: impl IntoIterator<Item = &'a str>) -> Self {
        let mut filter_set = FilterSet::new(false);
        for list in lists {
            filter_set.add_filter_list(list.to_owned(), ParseOptions::default());
        }
        Blocker {
            engine: Engine::new_with_filter_set(filter_set),
        }
    }

    /// Build a blocker from the filter lists bundled with the browser.
    pub fn with_default_lists() -> Self {
        Self::from_filter_lists(crate::default_filter_lists())
    }

    /// Decide whether a request should be blocked.
    ///
    /// `source_url` is the URL of the page issuing the request (used for
    /// first/third-party discrimination); pass the request URL itself when no
    /// source is known. Top-level document loads are never blocked: the user
    /// asked for them, and network filters target subresources.
    pub fn should_block(&self, url: &str, source_url: Option<&str>, kind: RequestKind) -> bool {
        if kind == RequestKind::Document {
            return false;
        }
        let source = source_url.unwrap_or(url);
        match Request::new(url, source, kind.as_adblock_type(), "GET") {
            Ok(request) => self.engine.check_network_request(&request).should_block(),
            Err(error) => {
                log::debug!("unparseable request url {url}: {error}");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocker() -> Blocker {
        Blocker::with_default_lists()
    }

    #[test]
    fn blocks_known_trackers() {
        let blocker = blocker();
        for (url, kind) in [
            (
                "https://www.google-analytics.com/analytics.js",
                RequestKind::Script,
            ),
            (
                "https://connect.facebook.net/en_US/fbevents.js",
                RequestKind::Script,
            ),
            (
                "https://securepubads.g.doubleclick.net/tag/js/gpt.js",
                RequestKind::Script,
            ),
        ] {
            assert!(
                blocker.should_block(url, Some("https://example.com/"), kind),
                "expected {url} to be blocked"
            );
        }
    }

    #[test]
    fn allows_ordinary_content() {
        let blocker = blocker();
        for (url, kind) in [
            ("https://example.com/style.css", RequestKind::Style),
            (
                "https://upload.wikimedia.org/wikipedia/commons/a/a9/Example.jpg",
                RequestKind::Image,
            ),
            ("https://duckduckgo.com/dist/b.js", RequestKind::Script),
        ] {
            assert!(
                !blocker.should_block(url, Some("https://example.com/"), kind),
                "expected {url} to be allowed"
            );
        }
    }

    #[test]
    fn never_blocks_top_level_documents() {
        let blocker = blocker();
        assert!(!blocker.should_block(
            "https://www.google-analytics.com/analytics.js",
            None,
            RequestKind::Document
        ));
    }

    #[test]
    fn tolerates_garbage_urls() {
        let blocker = blocker();
        assert!(!blocker.should_block("not a url", None, RequestKind::Script));
    }
}
