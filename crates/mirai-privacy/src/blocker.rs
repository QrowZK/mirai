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

    /// Like [`Blocker::with_default_lists`], but caches the compiled engine in
    /// `cache_dir` so subsequent startups skip filter-list parsing. The cache
    /// is keyed by the content of the bundled lists, so updating them
    /// invalidates it automatically.
    pub fn with_default_lists_cached(cache_dir: &std::path::Path) -> Self {
        let hash = crate::default_filter_lists().fold(0xcbf29ce484222325u64, |hash, list| {
            fnv1a(hash, list.as_bytes())
        });
        let cache_file = cache_dir.join(format!("blocker-{hash:016x}.bin"));

        if let Ok(serialized) = std::fs::read(&cache_file) {
            let mut engine = Engine::default();
            if engine.deserialize(&serialized).is_ok() {
                return Blocker { engine };
            }
            log::warn!("stale or corrupt blocker cache, rebuilding");
        }

        let blocker = Self::with_default_lists();
        if std::fs::create_dir_all(cache_dir)
            .and_then(|_| std::fs::write(&cache_file, blocker.engine.serialize()))
            .is_err()
        {
            log::warn!("could not write blocker cache to {}", cache_file.display());
        }
        blocker
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

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
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
    fn cache_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mirai-blocker-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        // First call builds and writes the cache; second call loads it.
        let first = Blocker::with_default_lists_cached(&dir);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let second = Blocker::with_default_lists_cached(&dir);

        let url = "https://www.google-analytics.com/analytics.js";
        for blocker in [&first, &second] {
            assert!(blocker.should_block(url, Some("https://example.com/"), RequestKind::Script));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tolerates_garbage_urls() {
        let blocker = blocker();
        assert!(!blocker.should_block("not a url", None, RequestKind::Script));
    }
}
