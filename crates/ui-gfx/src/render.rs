//! Draw the menu to any `Canvas` (report §4.5), styled to match the design in
//! docs/diagrams/fig35.svg: a dark gradient, a brand top bar, rounded entry
//! cards with a selection highlight + star + health line, and — on wide screens —
//! a detail panel for the selected entry. Pure w.r.t. firmware: it only calls
//! `Canvas` methods, so it is host-tested against a pixel-buffer canvas.
extern crate alloc;
use crate::canvas::{Canvas, Rgb};
use crate::menu::{HealthTag, Menu, Row};
use crate::{font, theme};
use alloc::string::String;

/// Render a full frame. `countdown` is the remaining auto-boot seconds (shown in
/// the keybar pill), or `None` once the countdown has been cancelled by input.
pub fn draw(canvas: &mut dyn Canvas, menu: &Menu, title: &str, countdown: Option<u32>) {
    let (w, h) = canvas.dimensions();
    canvas.fill_vgradient(theme::BG_TOP, theme::BG_BOT);

    // ---- top bar: brand dot + title + right-aligned status ----
    canvas.disc(theme::MARGIN + 6, 40, 14, theme::ACCENT);
    draw_text_scaled(
        canvas,
        theme::MARGIN + 28,
        28,
        "MyBoot",
        theme::TEXT,
        theme::TITLE_SCALE,
    );
    let n = menu.rows().len();
    let status = alloc::format!("UEFI \u{2022} {n} entr{}", if n == 1 { "y" } else { "ies" });
    draw_text_right(
        canvas,
        w.saturating_sub(theme::MARGIN),
        34,
        &status,
        theme::SUB,
        theme::BODY_SCALE,
    );
    canvas.fill_rect(
        theme::MARGIN - 10,
        theme::TOPBAR_H,
        w.saturating_sub(2 * (theme::MARGIN - 10)),
        2,
        theme::DIVIDER,
    );

    draw_text_scaled(
        canvas,
        theme::MARGIN,
        theme::TOPBAR_H + 20,
        title,
        theme::SUB,
        theme::BODY_SCALE,
    );

    // ---- layout: cards on the left, a detail panel on the right (wide screens) ----
    let two_col = w >= 1000;
    let left_w = if two_col {
        (w * 11) / 20
    } else {
        w.saturating_sub(2 * theme::MARGIN)
    };
    let card_x = theme::MARGIN;
    let card_w = left_w.saturating_sub(theme::MARGIN / 2);
    let list_top = theme::TOPBAR_H + 52;
    let keybar_h = 74;
    let list_bottom = h.saturating_sub(keybar_h);

    for (i, row) in menu.rows().iter().enumerate() {
        let y = list_top + i * (theme::CARD_H + theme::CARD_GAP);
        if y + theme::CARD_H > list_bottom {
            break;
        } // don't collide with the keybar
        draw_card(canvas, card_x, y, card_w, row, i == menu.cursor());
    }

    if two_col {
        let panel_x = theme::MARGIN + left_w;
        let panel_w = w.saturating_sub(panel_x).saturating_sub(theme::MARGIN);
        draw_detail_panel(
            canvas,
            panel_x,
            list_top,
            panel_w,
            list_bottom.saturating_sub(list_top + 12),
            menu.selected(),
        );
    }

    draw_keybar(canvas, w, h, countdown);
}

/// The footer hotkey bar (fig35): key-caps + labels, plus a live auto-boot pill.
fn draw_keybar(canvas: &mut dyn Canvas, w: usize, h: usize, countdown: Option<u32>) {
    if h < theme::TOPBAR_H + 70 {
        return;
    }
    let s = theme::SMALL_SCALE; // compact keybar text, like the design
    let y = h.saturating_sub(44);
    canvas.fill_rect(
        theme::MARGIN.saturating_sub(10),
        y.saturating_sub(14),
        w.saturating_sub(2 * theme::MARGIN.saturating_sub(10)),
        2,
        theme::DIVIDER,
    );
    let ky = y + 6;

    // right-aligned auto-boot pill first, so keys never overlap it
    let mut pill_left = w.saturating_sub(theme::MARGIN);
    if let Some(secs) = countdown {
        let label = alloc::format!("auto-boot {secs}s");
        let pill_w = label.chars().count() * (font::GLYPH_W + 1) * theme::BODY_SCALE + 24;
        let px = w.saturating_sub(theme::MARGIN).saturating_sub(pill_w);
        canvas.card(px, ky - 6, pill_w, 26, theme::SEL, theme::ACCENT, 1);
        draw_text_scaled(
            canvas,
            px + 12,
            ky - 1,
            &label,
            theme::TEXT,
            theme::BODY_SCALE,
        );
        pill_left = px.saturating_sub(16);
    }

    let keys = [
        ("\u{2195}", "select"),
        ("\u{23CE}", "boot"),
        ("E", "edit"),
        ("R", "recovery"),
        ("F", "firmware"),
        ("S", "shell"),
        ("Esc", "back"),
    ];
    let mut x = theme::MARGIN;
    for (key, label) in keys {
        let next = key_cap(canvas, x, ky, key, label, s);
        if next > pill_left {
            break;
        } // stop before the pill
        x = next;
    }
}

/// Draw a key-cap `[key] label` at `scale` and return the x after the label.
fn key_cap(
    canvas: &mut dyn Canvas,
    x: usize,
    y: usize,
    key: &str,
    label: &str,
    scale: usize,
) -> usize {
    let adv = (font::GLYPH_W + 1) * scale;
    let cap_w = key.chars().count() * adv + 12;
    canvas.card(
        x,
        y - 2,
        cap_w,
        8 * scale + 8,
        theme::CARD,
        theme::CARD_BORDER,
        1,
    );
    draw_text_scaled(canvas, x + 6, y + 2, key, theme::TEXT, scale);
    let lx = x + cap_w + 6;
    draw_text_scaled(canvas, lx, y + 2, label, theme::MUT, scale);
    lx + label.chars().count() * adv + 18
}

/// Draw a button; filled (accent) or outlined. Returns the x after it.
fn button(
    canvas: &mut dyn Canvas,
    x: usize,
    y: usize,
    w: usize,
    text: &str,
    filled: bool,
) -> usize {
    if filled {
        canvas.card(x, y, w, 34, theme::ACCENT, theme::ACCENT, 1);
        draw_text_center(
            canvas,
            x + w / 2,
            y + 9,
            text,
            theme::BG_BOT,
            theme::BODY_SCALE,
        );
    } else {
        canvas.card(x, y, w, 34, theme::PANEL, theme::ACCENT, 2);
        draw_text_center(
            canvas,
            x + w / 2,
            y + 9,
            text,
            Rgb::new(0x9d, 0xc0, 0xff),
            theme::BODY_SCALE,
        );
    }
    x + w + 12
}

fn draw_card(canvas: &mut dyn Canvas, x: usize, y: usize, w: usize, row: &Row, selected: bool) {
    if selected {
        canvas.card(x, y, w, theme::CARD_H, theme::SEL, theme::ACCENT, 2);
        // accent spine + star
        canvas.fill_rect(x, y, 4, theme::CARD_H, theme::ACCENT);
        draw_text_scaled(
            canvas,
            x + 14,
            y + 16,
            "\u{2605}",
            theme::ACCENT,
            theme::HEAD_SCALE,
        );
    } else {
        canvas.card(x, y, w, theme::CARD_H, theme::CARD, theme::CARD_BORDER, 1);
    }

    let tx = x + 40;
    let text_w = w.saturating_sub(56);
    let title_color = if row.bootable {
        theme::TEXT
    } else {
        theme::MUT
    };
    draw_text_bold(
        canvas,
        tx,
        y + 14,
        &fit(&row.title, fit_chars(text_w, theme::HEAD_SCALE)),
        title_color,
        theme::HEAD_SCALE,
    );

    // subtitle: the OS/group name (falls back to a status word)
    let sub = if !row.os.is_empty() {
        row.os.clone()
    } else {
        String::from("boot entry")
    };
    draw_text_scaled(
        canvas,
        tx,
        y + 40,
        &fit(&sub, fit_chars(text_w, theme::BODY_SCALE)),
        theme::SUB,
        theme::BODY_SCALE,
    );

    // health line with a coloured mark
    let (mark, color, label) = health_line(row);
    draw_text_scaled(canvas, tx, y + 64, mark, color, theme::BODY_SCALE);
    draw_text_scaled(canvas, tx + 24, y + 64, label, color, theme::BODY_SCALE);
}

fn health_line(row: &Row) -> (&'static str, Rgb, &'static str) {
    if !row.bootable {
        return ("\u{2717}", theme::FAIL, "not bootable");
    }
    match row.health {
        HealthTag::Ok => ("\u{2713}", theme::OK, "healthy"),
        HealthTag::Warn => ("\u{26A0}", theme::WARN, "degraded"),
        HealthTag::Fail => ("\u{2717}", theme::FAIL, "unbootable"),
        HealthTag::Unknown => ("\u{2022}", theme::MUT, "unverified"),
    }
}

fn draw_detail_panel(
    canvas: &mut dyn Canvas,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    sel: Option<&Row>,
) {
    canvas.card(x, y, w, h, theme::PANEL, theme::CARD_BORDER, 1);
    let ix = x + 24;
    let inner = w.saturating_sub(48); // usable text width
    let title_max = fit_chars(inner, theme::HEAD_SCALE);
    let val_label_gap = 8 * (font::GLYPH_W + 1) * theme::BODY_SCALE; // reserve for the label
    let val_max = fit_chars(inner.saturating_sub(val_label_gap), theme::BODY_SCALE);
    match sel {
        None => {
            draw_text_scaled(
                canvas,
                ix,
                y + 24,
                "No entry selected",
                theme::SUB,
                theme::BODY_SCALE,
            );
        }
        Some(row) => {
            draw_text_bold(
                canvas,
                ix,
                y + 22,
                &fit(&row.title, title_max),
                theme::TEXT,
                theme::HEAD_SCALE,
            );
            canvas.fill_rect(ix, y + 50, inner, 1, theme::DIVIDER);
            let vx = x + w - 24;
            let mut ly = y + 66;
            kv(canvas, ix, vx, ly, "system", &fit(&row.os, val_max));
            ly += 28;
            let (_, hc, hl) = health_line(row);
            draw_text_scaled(canvas, ix, ly, "health", theme::MUT, theme::BODY_SCALE);
            draw_text_right(canvas, vx, ly, hl, hc, theme::BODY_SCALE);
            ly += 28;
            kv(
                canvas,
                ix,
                vx,
                ly,
                "action",
                if row.bootable { "boot" } else { "blocked" },
            );

            // health banner + Boot / Options buttons (fig35)
            let (_, hc2, hl2) = health_line(row);
            let banner_y = y + h.saturating_sub(96);
            if banner_y > ly + 20 {
                canvas.card(ix, banner_y, inner, 28, theme::PANEL, hc2, 1);
                draw_text_scaled(
                    canvas,
                    ix + 12,
                    banner_y + 6,
                    &fit(hl2, fit_chars(inner.saturating_sub(24), theme::BODY_SCALE)),
                    hc2,
                    theme::BODY_SCALE,
                );
                let by = banner_y + 40;
                let bw = (inner.saturating_sub(12)) / 2;
                let x2 = button(canvas, ix, by, bw, "Boot", true);
                let _ = button(canvas, x2, by, bw, "Options", false);
            }
        }
    }
}

fn kv(canvas: &mut dyn Canvas, kx: usize, vx: usize, y: usize, k: &str, v: &str) {
    draw_text_scaled(canvas, kx, y, k, theme::MUT, theme::BODY_SCALE);
    draw_text_right(canvas, vx, y, v, theme::SUB, theme::BODY_SCALE);
}

/// How many glyphs fit in `w` pixels at `scale`.
fn fit_chars(w: usize, scale: usize) -> usize {
    let adv = (font::GLYPH_W + 1) * scale;
    w.checked_div(adv).unwrap_or(0)
}

/// Truncate `s` to at most `max` chars, adding an ellipsis when cut.
fn fit(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if max == 0 {
        return String::new();
    }
    if n <= max {
        return String::from(s);
    }
    let keep = max.saturating_sub(1).max(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('\u{2026}'); // …
    out
}

// ---------- text helpers ----------

/// Draw a string at (x, y) using the 8x8 font scaled by `scale`.
pub fn draw_text_scaled(
    canvas: &mut dyn Canvas,
    x: usize,
    y: usize,
    text: &str,
    color: Rgb,
    scale: usize,
) {
    let mut cx = x;
    for ch in text.chars() {
        let g = font::glyph(ch);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..font::GLYPH_W {
                if bits & (1 << col) != 0 {
                    canvas.fill_rect(cx + col * scale, y + row * scale, scale, scale, color);
                }
            }
        }
        cx += (font::GLYPH_W + 1) * scale; // 1px inter-glyph spacing, scaled
    }
}

/// Faux-bold: draw the text, then again shifted 1px right.
fn draw_text_bold(
    canvas: &mut dyn Canvas,
    x: usize,
    y: usize,
    text: &str,
    color: Rgb,
    scale: usize,
) {
    draw_text_scaled(canvas, x, y, text, color, scale);
    draw_text_scaled(canvas, x + 1, y, text, color, scale);
}

/// Centre text horizontally on `cx`.
fn draw_text_center(
    canvas: &mut dyn Canvas,
    cx: usize,
    y: usize,
    text: &str,
    color: Rgb,
    scale: usize,
) {
    let width = text.chars().count() * (font::GLYPH_W + 1) * scale;
    draw_text_scaled(canvas, cx.saturating_sub(width / 2), y, text, color, scale);
}

/// Right-align text so its end sits at `right_x`.
fn draw_text_right(
    canvas: &mut dyn Canvas,
    right_x: usize,
    y: usize,
    text: &str,
    color: Rgb,
    scale: usize,
) {
    let width = text.chars().count() * (font::GLYPH_W + 1) * scale;
    let x = right_x.saturating_sub(width);
    draw_text_scaled(canvas, x, y, text, color, scale);
}

/// Back-compat shim for older callers that used the 2x default.
pub fn draw_text(canvas: &mut dyn Canvas, x: usize, y: usize, text: &str, color: Rgb) {
    draw_text_scaled(canvas, x, y, text, color, theme::BODY_SCALE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{Canvas, Rgb};
    use crate::menu::Menu;
    use alloc::vec;
    use alloc::vec::Vec;
    use graph::{
        BootEntry, BootGraph, BootMethod, DiskId, DiskNode, EntryRole, Health, OsKind, OsNode,
    };

    struct Buf {
        w: usize,
        h: usize,
        px: Vec<Rgb>,
    }
    impl Buf {
        fn new(w: usize, h: usize) -> Self {
            Buf {
                w,
                h,
                px: vec![Rgb::new(0, 0, 0); w * h],
            }
        }
    }
    impl Canvas for Buf {
        fn dimensions(&self) -> (usize, usize) {
            (self.w, self.h)
        }
        fn put_pixel(&mut self, x: usize, y: usize, c: Rgb) {
            if x < self.w && y < self.h {
                self.px[y * self.w + x] = c;
            }
        }
    }

    fn sample() -> BootGraph {
        let mut g = BootGraph::new();
        let mut os = OsNode::new(OsKind::NixOs, "NixOS");
        let mut e = BootEntry::new(
            OsKind::NixOs,
            "NixOS Generation 23",
            EntryRole::Generation(23),
            BootMethod::NixGeneration,
        );
        e.health = Health::Healthy;
        os.push(e);
        let mut win = OsNode::new(OsKind::Windows, "Windows");
        let mut we = BootEntry::new(
            OsKind::Windows,
            "Windows Boot Manager",
            EntryRole::Default,
            BootMethod::WindowsBootManager,
        );
        we.health = Health::Healthy;
        win.push(we);
        g.add_disk(DiskNode {
            id: DiskId(0),
            label: "nvme0".into(),
            systems: vec![os, win],
        });
        g
    }

    #[test]
    fn draw_renders_content_without_panicking() {
        let g = sample();
        let menu = Menu::from_graph(&g);
        let mut buf = Buf::new(1180, 720);
        draw(&mut buf, &menu, "Select an operating system", Some(5));
        // the frame is not blank
        assert!(buf.px.iter().any(|p| *p != Rgb::new(0, 0, 0)));
        // the accent colour appears (selected spine/star/border)
        assert!(
            buf.px.iter().any(|p| *p == theme::ACCENT),
            "selection accent should be drawn"
        );
    }

    #[test]
    fn draw_is_total_on_a_tiny_canvas() {
        let g = sample();
        let menu = Menu::from_graph(&g);
        let mut buf = Buf::new(80, 40);
        draw(&mut buf, &menu, "x", None); // must not panic on a canvas smaller than the layout
    }

    #[test]
    fn symbols_have_glyphs() {
        for ch in ['\u{2605}', '\u{2713}', '\u{26A0}', '\u{2717}', '\u{2022}'] {
            assert!(
                font::glyph(ch).iter().any(|b| *b != 0),
                "symbol {ch:?} should have a glyph"
            );
        }
    }
}

/// Render the recovery screen (design: fig36): an amber header, a diagnosis list
/// with pass/fail marks and a recommended pick, and a diagnostics side panel.
pub fn draw_recovery(canvas: &mut dyn Canvas, rec: &crate::recovery::Recovery) {
    use crate::recovery::Recovery;
    let _: &Recovery = rec;
    let (w, h) = canvas.dimensions();
    canvas.fill_vgradient(theme::BG_TOP, theme::BG_BOT);

    // header (amber dot = attention)
    canvas.disc(theme::MARGIN + 6, 40, 14, theme::WARN);
    draw_text_scaled(
        canvas,
        theme::MARGIN + 28,
        28,
        "MyBoot \u{2014} Recovery",
        theme::TEXT,
        theme::TITLE_SCALE,
    );
    draw_text_right(
        canvas,
        w.saturating_sub(theme::MARGIN),
        34,
        "a previous boot did not complete",
        theme::SUB,
        theme::BODY_SCALE,
    );
    canvas.fill_rect(
        theme::MARGIN - 10,
        theme::TOPBAR_H,
        w.saturating_sub(2 * (theme::MARGIN - 10)),
        2,
        theme::DIVIDER,
    );

    let two_col = w >= 1000;
    let left_w = if two_col {
        (w * 11) / 20
    } else {
        w.saturating_sub(2 * theme::MARGIN)
    };

    // diagnosis panel
    draw_text_scaled(
        canvas,
        theme::MARGIN,
        theme::TOPBAR_H + 20,
        "Automatic diagnosis",
        theme::SUB,
        theme::BODY_SCALE,
    );
    let dx = theme::MARGIN;
    let dy = theme::TOPBAR_H + 44;
    let dw = left_w.saturating_sub(theme::MARGIN / 2);
    let rows = rec.items().len().min(12);
    let dh = (rows.max(1) * 30 + 28).min(h.saturating_sub(dy + 70));
    canvas.card(dx, dy, dw, dh, theme::PANEL, theme::CARD_BORDER, 1);

    let adv = (font::GLYPH_W + 1) * theme::BODY_SCALE;
    let tag_px = "recommended".len() * adv + 20; // reserved on recommended rows
    let title_max = fit_chars(dw.saturating_sub(64), theme::BODY_SCALE);
    let title_max_rec = fit_chars(dw.saturating_sub(64 + tag_px), theme::BODY_SCALE);
    for (i, it) in rec.items().iter().take(rows).enumerate() {
        let ry = dy + 16 + i * 30;
        let selected = i == rec.cursor();
        if selected {
            canvas.fill_rect(dx + 6, ry - 4, dw.saturating_sub(12), 28, theme::SEL);
            canvas.fill_rect(dx + 6, ry - 4, 3, 28, theme::ACCENT);
        }
        let (mark, color) = if !it.bootable {
            ("\u{2717}", theme::FAIL)
        } else if it.ok {
            ("\u{2713}", theme::OK)
        } else {
            ("\u{26A0}", theme::WARN)
        };
        draw_text_scaled(canvas, dx + 18, ry, mark, color, theme::BODY_SCALE);
        let tcolor = if it.bootable { theme::TEXT } else { theme::MUT };
        let recommended = Some(i) == rec.recommended();
        let tmax = if recommended {
            title_max_rec
        } else {
            title_max
        };
        draw_text_scaled(
            canvas,
            dx + 44,
            ry,
            &fit(&it.title, tmax),
            tcolor,
            theme::BODY_SCALE,
        );
        if recommended {
            draw_text_right(
                canvas,
                dx + dw - 16,
                ry,
                "recommended",
                theme::OK,
                theme::BODY_SCALE,
            );
        }
    }

    // diagnostics side panel
    if two_col {
        let px = theme::MARGIN + left_w;
        let pw = w.saturating_sub(px).saturating_sub(theme::MARGIN);
        let ph = h.saturating_sub(dy + 70);
        canvas.card(px, dy, pw, ph, theme::PANEL, theme::CARD_BORDER, 1);
        draw_text_bold(
            canvas,
            px + 24,
            dy + 22,
            "Boot diagnostics",
            theme::TEXT,
            theme::HEAD_SCALE,
        );
        canvas.fill_rect(px + 24, dy + 50, pw.saturating_sub(48), 1, theme::DIVIDER);
        let vmax = fit_chars(
            pw.saturating_sub(48 + 10 * (font::GLYPH_W + 1) * theme::BODY_SCALE),
            theme::BODY_SCALE,
        );
        let mut ly = dy + 66;
        for (k, v) in rec.diagnostics() {
            draw_text_scaled(canvas, px + 24, ly, k, theme::MUT, theme::BODY_SCALE);
            draw_text_right(
                canvas,
                px + pw - 24,
                ly,
                &fit(v, vmax),
                theme::SUB,
                theme::BODY_SCALE,
            );
            ly += 28;
            if ly > dy + ph - 20 {
                break;
            }
        }
    }

    // hint bar
    if h > theme::TOPBAR_H + 60 {
        let hint_y = h.saturating_sub(34);
        draw_text_scaled(
            canvas,
            theme::MARGIN,
            hint_y,
            "UP/DN select   ENTER boot   ESC back to menu",
            theme::MUT,
            theme::BODY_SCALE,
        );
    }
}
