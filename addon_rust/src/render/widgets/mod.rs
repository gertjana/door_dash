//! Per-widget rendering.
//!
//! Each submodule exposes a free `render(...)` function that draws a
//! single widget into a target [`Rect`] on the parent canvas. This
//! mirrors the Python addon's contract from
//! `addon/app/render/widgets/base.py` — there's nothing dynamic about
//! widget dispatch, so we use plain functions instead of a trait.
//!
//! Widgets MUST stay inside their target rectangle. They take their
//! data payload (already fetched by the corresponding `sources::*`
//! module) plus whatever slice of [`Settings`](crate::config::Settings)
//! they need to read user options.

pub mod base;
pub mod calendar_list;
pub mod local_sensors;
pub mod qr;
pub mod tesla;
pub mod weather;

pub use base::Rect;
