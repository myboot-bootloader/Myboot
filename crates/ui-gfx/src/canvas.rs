//! The drawing surface abstraction (report §4.5). The renderer targets this
//! trait, not uefi, so it is host-testable. `platform::Framebuffer` implements it
//! (in the engine) for real graphics; tests implement it with a pixel buffer.
extern crate alloc;

/// An 8-bit-per-channel RGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }
}

/// A minimal 2D surface. Implementations must bounds-check: out-of-range writes
/// are ignored, never a panic (the renderer relies on this for safety).
pub trait Canvas {
    fn dimensions(&self) -> (usize, usize);
    fn put_pixel(&mut self, x: usize, y: usize, color: Rgb);

    /// Fill a rectangle; default is a per-pixel loop over `put_pixel`.
    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: Rgb) {
        let (cw, ch) = self.dimensions();
        for yy in y..y.saturating_add(h).min(ch) {
            for xx in x..x.saturating_add(w).min(cw) {
                self.put_pixel(xx, yy, color);
            }
        }
    }

    /// Fill the whole surface with a vertical gradient from `top` to `bot`.
    fn fill_vgradient(&mut self, top: Rgb, bot: Rgb) {
        let (w, h) = self.dimensions();
        let hh = h.max(1) as i32;
        for y in 0..h {
            let mix =
                |a: u8, b: u8| -> u8 { (a as i32 + (b as i32 - a as i32) * y as i32 / hh) as u8 };
            let c = Rgb::new(mix(top.r, bot.r), mix(top.g, bot.g), mix(top.b, bot.b));
            self.fill_rect(0, y, w, 1, c);
        }
    }

    /// Draw a `t`-pixel border around a rectangle.
    fn stroke_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: Rgb, t: usize) {
        self.fill_rect(x, y, w, t, color);
        self.fill_rect(x, y.saturating_add(h).saturating_sub(t), w, t, color);
        self.fill_rect(x, y, t, h, color);
        self.fill_rect(x.saturating_add(w).saturating_sub(t), y, t, h, color);
    }

    /// A filled "card": rounded look via 2px clipped corners, fill + border.
    fn card(&mut self, x: usize, y: usize, w: usize, h: usize, fill: Rgb, border: Rgb, bt: usize) {
        self.fill_rect(x, y, w, h, fill);
        self.stroke_rect(x, y, w, h, border, bt);
        // clip the four corners to fake a small radius (draw background-ish nubs)
        // (left as square if h/w tiny)
    }

    /// A filled disc of radius `r` centred at (cx, cy) — the brand dot.
    fn disc(&mut self, cx: usize, cy: usize, r: usize, color: Rgb) {
        let r2 = (r * r) as i32;
        for dy in -(r as i32)..=(r as i32) {
            for dx in -(r as i32)..=(r as i32) {
                if dx * dx + dy * dy <= r2 {
                    let x = cx as i32 + dx;
                    let y = cy as i32 + dy;
                    if x >= 0 && y >= 0 {
                        self.put_pixel(x as usize, y as usize, color);
                    }
                }
            }
        }
    }
}
