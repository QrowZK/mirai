/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::Cell;

use dom_struct::dom_struct;
use js::context::JSContext;
use script_bindings::reflector::reflect_dom_object_with_cx;
use stylo_atoms::Atom;

use crate::dom::bindings::buffer_source::get_buffer_source_slice;
use crate::dom::bindings::codegen::Bindings::MediaSourceBinding::MediaSourceReadyState;
use crate::dom::bindings::codegen::Bindings::HTMLMediaElementBinding::HTMLMediaElementMethods;
use crate::dom::bindings::codegen::Bindings::SourceBufferBinding::{
    AppendMode, SourceBufferMethods,
};
use crate::dom::bindings::codegen::UnionTypes::ArrayBufferViewOrArrayBuffer;
use crate::dom::bindings::error::{Error, ErrorResult};
use script_bindings::num::Finite;
use crate::dom::bindings::refcounted::Trusted;
use crate::dom::bindings::reflector::DomGlobal;
use crate::dom::bindings::root::{Dom, DomRoot};
use crate::dom::bindings::inheritance::Castable;
use crate::dom::eventtarget::EventTarget;
use crate::dom::globalscope::GlobalScope;
use crate::dom::mediasource::MediaSource;
use crate::dom::timeranges::{TimeRanges, TimeRangesContainer};
use crate::dom::window::Window;

/// <https://w3c.github.io/media-source/#sourcebuffer>
#[dom_struct]
pub(crate) struct SourceBuffer {
    eventtarget: EventTarget,
    media_source: Dom<MediaSource>,
    mime_type: String,
    mode: Cell<AppendMode>,
    updating: Cell<bool>,
    timestamp_offset: Cell<f64>,
    append_window_start: Cell<f64>,
    append_window_end: Cell<f64>,
}

impl SourceBuffer {
    fn new_inherited(media_source: &MediaSource, mime_type: &str) -> SourceBuffer {
        SourceBuffer {
            eventtarget: EventTarget::new_inherited(),
            media_source: Dom::from_ref(media_source),
            mime_type: mime_type.to_owned(),
            mode: Cell::new(AppendMode::Segments),
            updating: Cell::new(false),
            timestamp_offset: Cell::new(0.),
            append_window_start: Cell::new(0.),
            append_window_end: Cell::new(f64::INFINITY),
        }
    }

    pub(crate) fn new(
        cx: &mut JSContext,
        global: &GlobalScope,
        media_source: &MediaSource,
        mime_type: &str,
    ) -> DomRoot<SourceBuffer> {
        reflect_dom_object_with_cx(
            Box::new(SourceBuffer::new_inherited(media_source, mime_type)),
            global,
            cx,
        )
    }
}

impl SourceBufferMethods<crate::DomTypeHolder> for SourceBuffer {
    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-mode>
    fn Mode(&self) -> AppendMode {
        self.mode.get()
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-mode>
    fn SetMode(&self, mode: AppendMode) -> ErrorResult {
        // MVP: only "segments" mode is supported.
        if mode != AppendMode::Segments {
            return Err(Error::NotSupported(None));
        }
        Ok(())
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-updating>
    fn Updating(&self) -> bool {
        self.updating.get()
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-buffered>
    fn Buffered(&self, cx: &mut JSContext) -> DomRoot<TimeRanges> {
        // MVP: proxy the attached media element's buffered ranges, which come
        // from the underlying player.
        if let Some(element) = self.media_source.attached_element() {
            return element.Buffered(cx);
        }
        TimeRanges::new(cx, self.global().as_window(), TimeRangesContainer::default())
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-timestampoffset>
    fn TimestampOffset(&self) -> Finite<f64> {
        Finite::wrap(self.timestamp_offset.get())
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-timestampoffset>
    fn SetTimestampOffset(&self, value: Finite<f64>) -> ErrorResult {
        self.timestamp_offset.set(*value);
        Ok(())
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-appendwindowstart>
    fn AppendWindowStart(&self) -> Finite<f64> {
        Finite::wrap(self.append_window_start.get())
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-appendwindowstart>
    fn SetAppendWindowStart(&self, value: Finite<f64>) -> ErrorResult {
        self.append_window_start.set(*value);
        Ok(())
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-appendwindowend>
    fn AppendWindowEnd(&self) -> f64 {
        self.append_window_end.get()
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-appendwindowend>
    fn SetAppendWindowEnd(&self, value: f64) -> ErrorResult {
        self.append_window_end.set(value);
        Ok(())
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-appendbuffer>
    fn AppendBuffer(&self, cx: &mut JSContext, data: ArrayBufferViewOrArrayBuffer) -> ErrorResult {
        // Step 1 (prepare append): if updating is true, throw InvalidStateError.
        if self.updating.get() {
            return Err(Error::InvalidState(None));
        }
        // Step 2 (prepare append): if the readyState is "ended", transition
        // back to "open".
        self.media_source.reopen_if_ended();
        if self.media_source.ready_state() != MediaSourceReadyState::Open {
            return Err(Error::InvalidState(None));
        }

        // Copy out the bytes before returning to script.
        let bytes = get_buffer_source_slice(&data, cx.no_gc()).to_vec();

        // Step 2. Set the updating attribute to true.
        self.updating.set(true);

        // Steps 3-4. Queue a task to fire updatestart, then run the buffer
        // append algorithm (here: feed the bytes to the media pipeline).
        let this = Trusted::new(self);
        self.global()
            .task_manager()
            .media_element_task_source()
            .queue(task!(source_buffer_append: move |cx| {
                let this = this.root();
                let target = this.upcast::<EventTarget>();
                target.fire_event(cx, Atom::from("updatestart"));

                let pushed = this
                    .media_source
                    .attached_element()
                    .map_or(Err(()), |element| element.push_media_source_data(bytes));

                this.updating.set(false);
                match pushed {
                    Ok(()) => {
                        target.fire_event(cx, Atom::from("update"));
                        target.fire_event(cx, Atom::from("updateend"));
                    },
                    Err(()) => {
                        target.fire_event(cx, Atom::from("error"));
                        target.fire_event(cx, Atom::from("updateend"));
                    },
                }
            }));
        Ok(())
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-abort>
    fn Abort(&self) -> ErrorResult {
        if self.media_source.ready_state() != MediaSourceReadyState::Open {
            return Err(Error::InvalidState(None));
        }
        // MVP: appends are atomic (whole payload is pushed in the queued
        // task), so there is nothing in-flight to truncate.
        if self.updating.get() {
            self.updating.set(false);
            let this = Trusted::new(self);
            self.global()
                .task_manager()
                .media_element_task_source()
                .queue(task!(source_buffer_abort: move |cx| {
                    let this = this.root();
                    let target = this.upcast::<EventTarget>();
                    target.fire_event(cx, Atom::from("abort"));
                    target.fire_event(cx, Atom::from("updateend"));
                }));
        }
        Ok(())
    }

    /// <https://w3c.github.io/media-source/#dom-sourcebuffer-remove>
    fn Remove(&self, start: Finite<f64>, end: f64) -> ErrorResult {
        let start = *start;
        if self.updating.get() {
            return Err(Error::InvalidState(None));
        }
        if !start.is_finite() || start < 0. || end <= start {
            return Err(Error::Type(c"Invalid remove range".to_owned()));
        }
        // MVP: removal from the pipeline is not supported; complete the async
        // protocol so callers relying on updateend make progress.
        self.updating.set(true);
        let this = Trusted::new(self);
        self.global()
            .task_manager()
            .media_element_task_source()
            .queue(task!(source_buffer_remove: move |cx| {
                let this = this.root();
                this.updating.set(false);
                let target = this.upcast::<EventTarget>();
                target.fire_event(cx, Atom::from("update"));
                target.fire_event(cx, Atom::from("updateend"));
            }));
        Ok(())
    }

    event_handler!(updatestart, GetOnupdatestart, SetOnupdatestart);
    event_handler!(update, GetOnupdate, SetOnupdate);
    event_handler!(updateend, GetOnupdateend, SetOnupdateend);
    event_handler!(error, GetOnerror, SetOnerror);
    event_handler!(abort, GetOnabort, SetOnabort);
}
