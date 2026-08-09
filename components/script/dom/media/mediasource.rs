/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use dom_struct::dom_struct;
use js::context::JSContext;
use js::rust::HandleObject;
use script_bindings::reflector::reflect_dom_object_with_proto_and_cx;
use servo_url::ServoUrl;
use stylo_atoms::Atom;
use uuid::Uuid;

use crate::dom::bindings::codegen::Bindings::MediaSourceBinding::{
    EndOfStreamError, MediaSourceMethods, MediaSourceReadyState,
};
use crate::dom::bindings::error::{Error, ErrorResult, Fallible};
use crate::dom::bindings::refcounted::Trusted;
use crate::dom::bindings::reflector::DomGlobal;
use crate::dom::bindings::root::{DomRoot, MutNullableDom};
use crate::dom::bindings::str::DOMString;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::eventtarget::EventTarget;
use crate::dom::window::Window;
use crate::dom::htmlmediaelement::HTMLMediaElement;
use crate::dom::sourcebuffer::SourceBuffer;
use crate::dom::sourcebufferlist::SourceBufferList;

thread_local! {
    /// Registry mapping object URLs created via `URL.createObjectURL(mediaSource)`
    /// to their [`MediaSource`] objects. `Trusted` keeps the object alive until
    /// the URL is revoked (or the script thread dies).
    static MEDIA_SOURCE_URL_REGISTRY: RefCell<HashMap<String, Trusted<MediaSource>>> =
        RefCell::new(HashMap::new());
}

/// <https://w3c.github.io/media-source/#mediasource>
#[dom_struct]
pub(crate) struct MediaSource {
    eventtarget: EventTarget,
    ready_state: Cell<MediaSourceReadyState>,
    duration: Cell<f64>,
    source_buffers: MutNullableDom<SourceBufferList>,
    /// The media element this source is currently attached to.
    attached_element: MutNullableDom<HTMLMediaElement>,
}

impl MediaSource {
    fn new_inherited() -> MediaSource {
        MediaSource {
            eventtarget: EventTarget::new_inherited(),
            ready_state: Cell::new(MediaSourceReadyState::Closed),
            duration: Cell::new(f64::NAN),
            source_buffers: Default::default(),
            attached_element: Default::default(),
        }
    }

    fn new(
        cx: &mut JSContext,
        window: &Window,
        proto: Option<HandleObject>,
    ) -> DomRoot<MediaSource> {
        reflect_dom_object_with_proto_and_cx(
            Box::new(MediaSource::new_inherited()),
            window,
            proto,
            cx,
        )
    }

    pub(crate) fn ready_state(&self) -> MediaSourceReadyState {
        self.ready_state.get()
    }

    pub(crate) fn attached_element(&self) -> Option<DomRoot<HTMLMediaElement>> {
        self.attached_element.get()
    }

    /// Register this [`MediaSource`] under a newly minted object URL and
    /// return that URL.
    pub(crate) fn create_object_url(&self) -> String {
        let origin = self.global().origin().immutable().ascii_serialization();
        let url = format!("blob:{}/{}", origin, Uuid::new_v4());
        MEDIA_SOURCE_URL_REGISTRY.with(|registry| {
            registry
                .borrow_mut()
                .insert(url.clone(), Trusted::new(self));
        });
        url
    }

    /// Look up a URL in the [`MediaSource`] object URL registry.
    pub(crate) fn lookup_url(url: &ServoUrl) -> Option<DomRoot<MediaSource>> {
        MEDIA_SOURCE_URL_REGISTRY.with(|registry| {
            registry
                .borrow()
                .get(url.as_str())
                .map(|trusted| trusted.root())
        })
    }

    /// Remove a URL from the registry. Returns true if it was present.
    pub(crate) fn revoke_url(url: &str) -> bool {
        MEDIA_SOURCE_URL_REGISTRY.with(|registry| registry.borrow_mut().remove(url).is_some())
    }

    /// <https://w3c.github.io/media-source/#mediasource-attachment>
    /// Called from the media element's resource fetch algorithm when its
    /// resource URL resolves to this [`MediaSource`].
    pub(crate) fn attach(&self, element: &HTMLMediaElement) {
        self.attached_element.set(Some(element));
        self.ready_state.set(MediaSourceReadyState::Open);
        self.queue_simple_event("sourceopen");
    }

    /// <https://w3c.github.io/media-source/#sourcebuffer-prepare-append>
    /// If the readyState is "ended", transition back to "open" (players
    /// append after endOfStream when seeking or looping).
    pub(crate) fn reopen_if_ended(&self) {
        if self.ready_state.get() == MediaSourceReadyState::Ended {
            self.ready_state.set(MediaSourceReadyState::Open);
            self.queue_simple_event("sourceopen");
        }
    }

    /// Queue a task to fire a simple event at this [`MediaSource`].
    fn queue_simple_event(&self, name: &'static str) {
        let this = Trusted::new(self);
        self.global()
            .task_manager()
            .media_element_task_source()
            .queue(task!(media_source_simple_event: move |cx| {
                let this = this.root();
                this.upcast::<EventTarget>().fire_event(cx, Atom::from(name));
            }));
    }

    fn is_type_supported(type_: &str) -> bool {
        // MVP: a single muxed (or single-track) buffer fed to GStreamer, which
        // handles the common WebM and ISO BMFF container formats.
        let type_ = type_.to_ascii_lowercase();
        ["video/webm", "audio/webm", "video/mp4", "audio/mp4"]
            .iter()
            .any(|supported| type_.starts_with(supported))
    }
}

impl MediaSourceMethods<crate::DomTypeHolder> for MediaSource {
    /// <https://w3c.github.io/media-source/#dom-mediasource-constructor>
    fn Constructor(
        cx: &mut JSContext,
        window: &Window,
        proto: Option<HandleObject>,
    ) -> DomRoot<MediaSource> {
        MediaSource::new(cx, window, proto)
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-sourcebuffers>
    fn SourceBuffers(&self, cx: &mut JSContext) -> DomRoot<SourceBufferList> {
        if let Some(list) = self.source_buffers.get() {
            return list;
        }
        let list = SourceBufferList::new(cx, &self.global());
        self.source_buffers.set(Some(&list));
        list
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-activesourcebuffers>
    fn ActiveSourceBuffers(&self, cx: &mut JSContext) -> DomRoot<SourceBufferList> {
        // MVP: all source buffers are active.
        self.SourceBuffers(cx)
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-readystate>
    fn ReadyState(&self) -> MediaSourceReadyState {
        self.ready_state.get()
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-duration>
    fn Duration(&self) -> f64 {
        self.duration.get()
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-duration>
    fn SetDuration(&self, value: f64) {
        self.duration.set(value);
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-addsourcebuffer>
    fn AddSourceBuffer(&self, cx: &mut JSContext, type_: DOMString) -> Fallible<DomRoot<SourceBuffer>> {
        // Step 1. If type is an empty string then throw a TypeError.
        if type_.is_empty() {
            return Err(Error::Type(c"Type must not be empty".to_owned()));
        }
        // Step 2. If type contains a MIME type that is not supported [...],
        // then throw a NotSupportedError.
        if !Self::is_type_supported(&type_.str()) {
            return Err(Error::NotSupported(None));
        }
        // Step 3 (MVP restriction): this implementation supports a single
        // SourceBuffer with muxed (or single-track) content.
        let list = self.SourceBuffers(cx);
        if list.len() > 0 {
            return Err(Error::QuotaExceeded {
                quota: None,
                requested: None,
            });
        }
        // Step 4. If the readyState attribute is not in the "open" state then
        // throw an InvalidStateError.
        if self.ready_state.get() != MediaSourceReadyState::Open {
            return Err(Error::InvalidState(None));
        }
        // Steps 5-7. Create the SourceBuffer and add it to sourceBuffers.
        let buffer = SourceBuffer::new(cx, &self.global(), self, &type_.str());
        list.add(&buffer);
        Ok(buffer)
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-removesourcebuffer>
    fn RemoveSourceBuffer(&self, source_buffer: &SourceBuffer) -> ErrorResult {
        if !self
            .source_buffers
            .get()
            .is_some_and(|list| list.remove(source_buffer))
        {
            return Err(Error::NotFound(None));
        }
        Ok(())
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-endofstream>
    fn EndOfStream(&self, _error: Option<EndOfStreamError>) -> ErrorResult {
        // Step 1. If the readyState attribute is not in the "open" state then
        // throw an InvalidStateError.
        if self.ready_state.get() != MediaSourceReadyState::Open {
            return Err(Error::InvalidState(None));
        }
        // Step 3. Run the end of stream algorithm.
        self.ready_state.set(MediaSourceReadyState::Ended);
        if let Some(element) = self.attached_element.get() {
            element.end_media_source_stream();
        }
        self.queue_simple_event("sourceended");
        Ok(())
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-setliveseekablerange>
    fn SetLiveSeekableRange(
        &self,
        _start: script_bindings::num::Finite<f64>,
        _end: script_bindings::num::Finite<f64>,
    ) -> ErrorResult {
        if self.ready_state.get() != MediaSourceReadyState::Open {
            return Err(Error::InvalidState(None));
        }
        // MVP: accepted but not used by the playback pipeline.
        Ok(())
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-clearliveseekablerange>
    fn ClearLiveSeekableRange(&self) -> ErrorResult {
        if self.ready_state.get() != MediaSourceReadyState::Open {
            return Err(Error::InvalidState(None));
        }
        Ok(())
    }

    /// <https://w3c.github.io/media-source/#dom-mediasource-istypesupported>
    fn IsTypeSupported(_window: &Window, type_: DOMString) -> bool {
        MediaSource::is_type_supported(&type_.str())
    }

    event_handler!(sourceopen, GetOnsourceopen, SetOnsourceopen);
    event_handler!(sourceended, GetOnsourceended, SetOnsourceended);
    event_handler!(sourceclose, GetOnsourceclose, SetOnsourceclose);
}
