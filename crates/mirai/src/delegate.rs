use std::sync::atomic::Ordering;

use content_security_policy::Destination;
use libservo::{WebResourceLoad, WebResourceResponse, WebView, WebViewDelegate};
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

    fn load_web_resource(&self, _webview: WebView, load: WebResourceLoad) {
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
            log::info!("blocked ({count}): {request_url}");
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
