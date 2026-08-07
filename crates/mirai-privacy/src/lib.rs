//! Privacy engine for the Mirai browser.
//!
//! This crate is deliberately independent of Servo so it can be built and
//! tested quickly on any platform. The browser hooks [`Blocker`] into Servo's
//! web-resource interception delegate to decide, per network request, whether
//! the request should be allowed to leave the machine.

mod blocker;
mod lists;

pub use blocker::{Blocker, RequestKind};
pub use lists::default_filter_lists;
