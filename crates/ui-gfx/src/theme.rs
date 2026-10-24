//! The colour palette and metrics for the graphical menu, matching the design in
//! docs/diagrams/fig35.svg. One cohesive place for visual constants (SRP).
use crate::canvas::Rgb;

// Background gradient (dark navy slate).
pub const BG_TOP: Rgb = Rgb::new(0x0d, 0x16, 0x26);
pub const BG_BOT: Rgb = Rgb::new(0x0a, 0x11, 0x20);
// Cards / panels.
pub const CARD: Rgb = Rgb::new(0x16, 0x23, 0x3b);
pub const CARD_BORDER: Rgb = Rgb::new(0x26, 0x37, 0x5a);
pub const SEL: Rgb = Rgb::new(0x18, 0x31, 0x53); // selected card fill
pub const PANEL: Rgb = Rgb::new(0x11, 0x1d, 0x31); // detail panel
pub const DIVIDER: Rgb = Rgb::new(0x22, 0x31, 0x4f);
// Text.
pub const TEXT: Rgb = Rgb::new(0xea, 0xf1, 0xfb);
pub const SUB: Rgb = Rgb::new(0x8f, 0xa1, 0xbc);
pub const MUT: Rgb = Rgb::new(0x6f, 0x82, 0xa0);
// Accents / status.
pub const ACCENT: Rgb = Rgb::new(0x4c, 0x8d, 0xff);
pub const OK: Rgb = Rgb::new(0x46, 0xc4, 0x6a);
pub const WARN: Rgb = Rgb::new(0xe0, 0xa6, 0x3a);
pub const FAIL: Rgb = Rgb::new(0xe5, 0x54, 0x4e);

// Back-compat aliases (older callers).
pub const BG: Rgb = BG_TOP;
pub const TEXT_DIM: Rgb = MUT;

// Metrics (source pixels; text is the 8x8 font scaled by these factors).
pub const MARGIN: usize = 40;
pub const TITLE_SCALE: usize = 3; // brand / big headings  (24px)
pub const HEAD_SCALE: usize = 2; // card titles           (16px)
pub const BODY_SCALE: usize = 2; // subtitles / details   (16px)
pub const SMALL_SCALE: usize = 1; // dense mono details    (8px)
pub const CARD_H: usize = 92;
pub const CARD_GAP: usize = 14;
pub const TOPBAR_H: usize = 82;
