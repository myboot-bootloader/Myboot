//! ui-gfx — the graphical boot menu (report §3.10.1, Figures 3.5–3.6). A driving
//! adapter over the Boot Graph. It is deliberately uefi-FREE: it draws to a
//! `Canvas` trait, so the menu model, layout, theme and text rendering are all
//! host-tested. The `engine` implements `Canvas` for `platform::Framebuffer`,
//! keeping the one firmware dependency at the composition root.
//!
//! The UI is NOT the boot architecture — it is only the presentation layer over
//! the boot-management core (per the project's own diagram).
#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

pub mod canvas;
pub mod font;
pub mod menu;
pub mod recovery;
pub mod render;
pub mod theme;

#[cfg(test)]
extern crate std;

pub use canvas::{Canvas, Rgb};
pub use menu::{InputEvent, Menu, MenuOutcome, Row};
