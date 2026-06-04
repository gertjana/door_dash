//! Data sources — async fetchers that hit Home Assistant (or, for
//! `local_sensors`, parse incoming query parameters) and return typed
//! data structures consumed by widgets.
//!
//! Each source mirrors `addon/app/sources/<name>.py`:
//!
//! * Defines a typed struct for the data shape (e.g. `Weather`,
//!   `TeslaState`).
//! * Exposes an async `fetch(settings)` function that constructs an
//!   `HAClient` internally, performs the requests, and returns the
//!   typed value.
//! * Falls back to synthesized demo data when HA is unreachable so
//!   dev runs without a HA backend still render plausibly.
//!
//! Sources never panic on bad data — they substitute `None` /
//! fallback values and let the render layer decide what to display.

pub mod calendar;
pub mod energy;
pub mod local_sensors;
pub mod tesla;
pub mod weather;
