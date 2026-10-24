//! Graphics Output Protocol acquisition and double-buffered presentation
//! (report §4.5). Provides a linear back buffer the UI renders into, then blits
//! it to the framebuffer with `blt` — the approach used by graphical EFI menus.
extern crate alloc;
use alloc::vec::Vec;
use ports::{IoError, IoResult};
use uefi::boot::{self, ScopedProtocol};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput};

/// A CPU-side back buffer plus the live GOP handle. The UI draws into `back`,
/// then calls `present` to blit the whole frame (or a sub-rectangle) to screen.
pub struct Framebuffer {
    gop: ScopedProtocol<GraphicsOutput>,
    back: Vec<BltPixel>,
    width: usize,
    height: usize,
}

impl Framebuffer {
    /// Acquire the GOP for the console and allocate a matching back buffer.
    pub fn acquire() -> IoResult<Framebuffer> {
        let handle =
            boot::get_handle_for_protocol::<GraphicsOutput>().map_err(|e| map_err(e.status()))?;
        // No `mut` needed: the local is only read here (current_mode_info takes
        // &self) and then moved into the struct; the mutable `blt` happens later
        // through `&mut self` in `present()`.
        let gop = boot::open_protocol_exclusive::<GraphicsOutput>(handle)
            .map_err(|e| map_err(e.status()))?;
        let (width, height) = gop.current_mode_info().resolution();
        let back = alloc::vec![BltPixel::new(0, 0, 0); width * height];
        Ok(Framebuffer {
            gop,
            back,
            width,
            height,
        })
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Mutable access to the back buffer for the renderer (row-major, RGB).
    pub fn buffer_mut(&mut self) -> &mut [BltPixel] {
        &mut self.back
    }

    /// Set one pixel (bounds-checked; out-of-range is ignored, never panics).
    pub fn put(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if x < self.width && y < self.height {
            self.back[y * self.width + x] = BltPixel::new(r, g, b);
        }
    }

    /// Fill the whole back buffer with one colour.
    pub fn clear(&mut self, r: u8, g: u8, b: u8) {
        for p in &mut self.back {
            *p = BltPixel::new(r, g, b);
        }
    }

    /// Blit the entire back buffer to the screen (one frame).
    pub fn present(&mut self) -> IoResult<()> {
        let dims = (self.width, self.height);
        self.gop
            .blt(BltOp::BufferToVideo {
                buffer: &self.back,
                src: BltRegion::Full,
                dest: (0, 0),
                dims,
            })
            .map_err(|e| map_err(e.status()))
    }
}

fn map_err(s: uefi::Status) -> IoError {
    IoError::Other(alloc::format!("gop status {s:?}"))
}
