use std::sync::atomic::Ordering;

use content_security_policy::Destination;
use libservo::{
    EventLoopWaker, NavigationRequest, WebResourceLoad, WebResourceResponse, WebView,
    WebViewDelegate,
};
use mirai_privacy::RequestKind;

use crate::app::AppState;

impl WebViewDelegate for AppState {
    fn notify_new_frame_ready(&self, _webview: WebView) {
        self.window.request_redraw();
    }

    fn notify_page_title_changed(&self, webview: WebView, title: Option<String>) {
        if Some(webview.id()) == self.active_webview().map(|webview| webview.id()) {
            let title = title.unwrap_or_default();
            self.window.set_title(&format!("{title} — Mirai"));
        }
        self.window.request_redraw();
    }

    fn notify_url_changed(&self, _webview: WebView, _url: url::Url) {
        self.window.request_redraw();
    }

    fn notify_load_status_changed(&self, _webview: WebView, _status: libservo::LoadStatus) {
        self.window.request_redraw();
    }

    fn request_download(&self, _webview: WebView, url: url::Url, _suggested: Option<String>) {
        // The engine hit content it cannot render (e.g. an unknown content
        // type) and asks the chrome to download it instead.
        crate::downloads::start_download(
            url,
            self.profile.borrow().downloads_dir(),
            self.downloads.clone(),
            self.waker.clone_box(),
        );
        self.window.request_redraw();
    }

    fn request_navigation(&self, _webview: WebView, request: NavigationRequest) {
        // Servo has no embedder download support yet; navigations to file-type
        // URLs are denied and fetched by the chrome instead.
        if crate::downloads::is_download_url(&request.url) {
            let url = request.url.clone();
            request.deny();
            crate::downloads::start_download(
                url,
                self.profile.borrow().downloads_dir(),
                self.downloads.clone(),
                self.waker.clone_box(),
            );
            self.window.request_redraw();
            return;
        }
        request.allow();
    }

    fn load_web_resource(&self, webview: WebView, load: WebResourceLoad) {
        if !self.blocking_enabled.load(Ordering::Relaxed) {
            return;
        }
        let request = load.request();
        let request_url = request.url.clone();
        let source_url = request
            .referrer_url
            .as_ref()
            .map(|url| url.as_str().to_owned());
        let kind = destination_to_request_kind(request.destination, request.is_for_main_frame);

        if self
            .blocker
            .should_block(request_url.as_str(), source_url.as_deref(), kind)
        {
            let count = self.blocked_count.fetch_add(1, Ordering::Relaxed) + 1;
            *self
                .blocked_counts_per_tab
                .borrow_mut()
                .entry(webview.id())
                .or_insert(0) += 1;
            log::info!("blocked ({count}): {request_url}");
            self.window.request_redraw();
            // Answer the request locally with an immediately-cancelled empty
            // response so the request never reaches the network.
            load.intercept(WebResourceResponse::new(request_url))
                .cancel();
        }
    }
}

fn destination_to_request_kind(destination: Destination, is_for_main_frame: bool) -> RequestKind {
    match destination {
        Destination::Document => {
            if is_for_main_frame {
                RequestKind::Document
            } else {
                RequestKind::SubDocument
            }
        }
        Destination::Frame | Destination::IFrame | Destination::Embed | Destination::Object => {
            RequestKind::SubDocument
        }
        Destination::Script
        | Destination::ServiceWorker
        | Destination::SharedWorker
        | Destination::Worker
        | Destination::AudioWorklet
        | Destination::PaintWorklet
        | Destination::Xslt => RequestKind::Script,
        Destination::Style => RequestKind::Style,
        Destination::Image => RequestKind::Image,
        Destination::Font => RequestKind::Font,
        Destination::Audio | Destination::Video | Destination::Track => RequestKind::Media,
        Destination::Json
        | Destination::Manifest
        | Destination::Report
        | Destination::WebIdentity => RequestKind::Xhr,
        Destination::None => RequestKind::Xhr,
        #[allow(unreachable_patterns)]
        _ => RequestKind::Other,
    }
}
