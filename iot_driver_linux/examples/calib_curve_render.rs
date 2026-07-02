//! Render the screen-sync calibration curves off-screen and dump them as text,
//! to debug why the Canvas shows nothing in the real TUI. Reproduces the exact
//! layout path: a full terminal, the Device-Info horizontal split (62/38), then
//! render_preview's vertical split (curves + 1-line swatch) — at a realistic
//! non-zero offset. No hardware required.
//!
//! Run: `cargo run --example calib_curve_render`

use iot_driver::screen_calib::ColorCalibration;
use ratatui::backend::TestBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Color;
use ratatui::symbols;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratatui::widgets::{Block, Borders};
use ratatui::{Frame, Terminal};

/// Mirror of `screen::render_calibration_curves`, kept in sync for debugging.
/// `input` marks where a live color maps onto each curve.
fn draw_curves(f: &mut Frame, cal: ColorCalibration, input: Option<(u8, u8, u8)>, area: Rect) {
    const N: u32 = 48;
    let colors = [Color::Red, Color::Green, Color::Blue];
    let markers: Option<[(f64, f64); 3]> = input.map(|(r, g, b)| {
        let ins = [r, g, b];
        std::array::from_fn(|ch| (ins[ch] as f64, cal.channel_map(ins[ch], ch) as f64))
    });
    let canvas = Canvas::default()
        .block(Block::default().borders(Borders::ALL).title("Calibration"))
        .marker(symbols::Marker::Braille)
        .x_bounds([0.0, 255.0])
        .y_bounds([0.0, 255.0])
        .paint(move |ctx| {
            ctx.draw(&CanvasLine {
                x1: 0.0,
                y1: 0.0,
                x2: 255.0,
                y2: 255.0,
                color: Color::Gray,
            });
            for (ch, &color) in colors.iter().enumerate() {
                let mut prev = (0.0_f64, cal.channel_map(0, ch) as f64);
                for i in 1..=N {
                    let x = (i * 255 / N) as u8;
                    let cur = (x as f64, cal.channel_map(x, ch) as f64);
                    ctx.draw(&CanvasLine {
                        x1: prev.0,
                        y1: prev.1,
                        x2: cur.0,
                        y2: cur.1,
                        color,
                    });
                    prev = cur;
                }
            }
            if let Some(marks) = markers {
                const ARM: f64 = 7.0;
                for (ch, &(mx, my)) in marks.iter().enumerate() {
                    let color = colors[ch];
                    ctx.draw(&CanvasLine {
                        x1: mx - ARM,
                        y1: my,
                        x2: mx + ARM,
                        y2: my,
                        color,
                    });
                    ctx.draw(&CanvasLine {
                        x1: mx,
                        y1: my - ARM,
                        x2: mx,
                        y2: my + ARM,
                        color,
                    });
                    ctx.draw(&Points {
                        coords: &[(mx, my)],
                        color: Color::White,
                    });
                }
            }
        });
    f.render_widget(canvas, area);
}

fn dump(label: &str, w: u16, h: u16, place: impl Fn(&mut Frame) -> Rect) {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut drawn_area = Rect::default();
    term.draw(|f| {
        drawn_area = place(f);
    })
    .unwrap();
    let buf = term.backend().buffer().clone();
    let mut nonblank = 0usize;
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            let s = buf[(x, y)].symbol();
            if !s.is_empty() && s != " " {
                nonblank += 1;
            }
            out.push_str(if s.is_empty() { " " } else { s });
        }
        out.push('\n');
    }
    println!("=== {label} (canvas area = {drawn_area:?}, non-blank = {nonblank}) ===");
    println!("{out}");
}

fn main() {
    let identity = ColorCalibration::default();

    // 1. Identity curves, no marker (known-good baseline at offset 0,0).
    dump("A: identity, no marker", 44, 16, |f| {
        let area = f.area();
        draw_curves(f, identity, None, area);
        area
    });

    // 2. Exact TUI path: content area, 62/38 horizontal split, then render_preview's
    //    vertical split — canvas rendered into the right chunk at a non-zero offset.
    dump("B: TUI layout path", 90, 24, |f| {
        let full = f.area();
        let content = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(1),
            ])
            .split(full)[2];
        let right = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(content)[1];
        let rows = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).split(right);
        draw_curves(f, identity, None, rows[0]);
        rows[0]
    });

    // 3. Tweaked calibration + a live color marker (crosses where (200,128,64) maps).
    let tweaked = ColorCalibration {
        gain: [1.0, 1.0, 0.5],
        gamma: [2.0, 1.0, 1.0],
        saturation: 1.0,
    };
    dump("C: tweaked + marker @ (200,128,64)", 44, 16, |f| {
        let area = f.area();
        draw_curves(f, tweaked, Some((200, 128, 64)), area);
        area
    });
}
