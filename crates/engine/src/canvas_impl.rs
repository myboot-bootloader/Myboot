//! Bridge the pure `ui_gfx::Canvas` trait to the firmware `platform::Framebuffer`
//! (report §4.5). This is the single adapter point between the uefi-free UI and
//! real graphics — it belongs at the composition root, not in `ui-gfx`.
use platform::gop::Framebuffer;
use ui_gfx::canvas::{Canvas, Rgb};

/// Wraps a `Framebuffer` so the renderer can draw into it.
pub struct FbCanvas<'a> {
    pub fb: &'a mut Framebuffer,
}

impl<'a> Canvas for FbCanvas<'a> {
    fn dimensions(&self) -> (usize, usize) {
        self.fb.size()
    }
    fn put_pixel(&mut self, x: usize, y: usize, c: Rgb) {
        self.fb.put(x, y, c.r, c.g, c.b);
    }
}
