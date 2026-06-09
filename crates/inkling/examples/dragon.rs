//! The dragon demo.
//!
//!   cargo run --example dragon                       # live reveal in your terminal
//!   cargo run --example dragon -- --snapshots        # staged text frames (no TTY needed)
//!   cargo run --example dragon -- --art path/to.txt  # bring your own ASCII art
//!
//! By default it reveals a procedurally generated serpent, a single, perfectly
//! 8-connected stroke, so the geodesic spine-trace paints it tip-to-tip. Point
//! `--art` at any file to watch arbitrary art reveal (imperfect art leans on the
//! island fallback and still looks intentional).

use std::io::IsTerminal;

use inkling::{
    art::Art,
    frame,
    ordering::{Geodesic, GeodesicReport, Ordering},
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let snapshots = args.iter().any(|a| a == "--snapshots");

    let art = match arg_value(&args, "--art") {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(text) => Art::parse(&text),
            Err(e) => {
                eprintln!("inkling: could not read {path}: {e}");
                std::process::exit(1);
            }
        },
        None => Art::parse(&serpent(64, 13)),
    };

    let ordering = Geodesic::default();
    let GeodesicReport {
        ink_cells,
        connected_cells,
        spine_length,
    } = ordering.diagnose(&art);
    let ranks = ordering.rank(&art);

    eprintln!(
        "inkling · {ink_cells} ink cells · {connected_cells} on the spine \
         ({:.0}% connected) · spine length {spine_length}",
        100.0 * connected_cells as f32 / ink_cells.max(1) as f32,
    );

    // Headless / piped / explicit: print staged text frames and exit.
    if snapshots || !std::io::stdout().is_terminal() {
        for p in [0.0, 0.2, 0.4, 0.6, 0.8, 1.0] {
            println!("\n── progress {:>3.0}% {}", p * 100.0, "─".repeat(28));
            print!("{}", frame::to_string(&art, &ranks, p));
        }
        return;
    }

    #[cfg(feature = "terminal")]
    {
        use inkling::{
            easing::Easing,
            render::{animate, Style},
        };
        use std::time::Duration;
        if let Err(e) = animate(
            &art,
            &ranks,
            Style::default(),
            Duration::from_millis(3500),
            Easing::EaseInOutCubic,
        ) {
            eprintln!("inkling: render error: {e}");
        }
    }
}

/// Read the value following `key` in `args` (supports `--key value` and `--key=value`).
fn arg_value(args: &[String], key: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == key {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix(&format!("{key}=")) {
            return Some(v.to_string());
        }
    }
    None
}

/// Generate an Eastern-style serpent as a single 8-connected stroke: a sine-wave
/// body with crest spikes and a small head. Connectivity is guaranteed by
/// filling any vertical gap between adjacent columns, so the spine-trace reveals
/// it flawlessly from tail to head.
// Columns are scanned by index because each row `y` is *computed* from `x`
// (y = sine(x)); writing `grid[y][x]` is the clearest expression of that.
#[allow(clippy::needless_range_loop)]
fn serpent(width: usize, height: usize) -> String {
    use std::f32::consts::TAU;

    let amp = ((height as f32) - 3.0).max(1.0) / 2.0;
    let mid = (height as f32 - 1.0) / 2.0;
    let period = (width as f32) / 2.0; // two coils across the width

    let y_at = |x: usize| -> usize {
        let yf = mid - amp * ((x as f32) / period * TAU).sin();
        yf.round().clamp(0.0, height as f32 - 1.0) as usize
    };

    let mut grid = vec![vec![' '; width]; height];
    let mut prev_y = y_at(0);
    for x in 0..width {
        let y = y_at(x);
        // Bridge any vertical gap so the body is one continuous stroke.
        if x > 0 && y.abs_diff(prev_y) > 1 {
            for row in grid[y.min(prev_y)..=y.max(prev_y)].iter_mut() {
                if row[x] == ' ' {
                    row[x] = '|';
                }
            }
        }
        grid[y][x] = match prev_y {
            _ if x == 0 => '~',
            py if y < py => '/',
            py if y > py => '\\',
            _ => '~',
        };
        prev_y = y;
    }

    // Spikes on the crests (local highs), one row above the body.
    for x in 1..width.saturating_sub(1) {
        let y = y_at(x);
        if y > 0 && y < y_at(x - 1) && y <= y_at(x + 1) {
            grid[y - 1][x] = '^';
        }
    }

    // A small head at the leading (right) tip.
    let (hx, hy) = (width - 1, y_at(width - 1));
    grid[hy][hx] = '>';
    if hy > 0 {
        grid[hy - 1][hx.saturating_sub(1)] = 'o'; // eye
    }

    grid.into_iter()
        .map(|row| row.into_iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}
