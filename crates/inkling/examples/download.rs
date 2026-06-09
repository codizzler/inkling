//! A demo that reads like a real CLI tool installing something, with the dragon
//! reveal as the download step. Paced like a true transfer and made for recording.
//!
//!   cargo run --example download                          # glow, built-in dragon
//!   cargo run --example download -- rainbow               # rainbow palette
//!   cargo run --example download -- geodesic serpent.txt  # geodesic spine trace
//!   cargo run --example download -- rainbow art.txt       # rainbow, your own art

use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::{
    cursor::MoveTo,
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use inkling::{ordering::Geodesic, render::Style, Loader};

fn step(mark: &str, color: Color, msg: &str) {
    let _ = execute!(
        io::stdout(),
        Print("  "),
        SetForegroundColor(color),
        Print(mark),
        ResetColor,
        Print(format!(" {msg}\r\n"))
    );
}

fn pause(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rainbow = args.iter().any(|a| a == "rainbow");
    let geodesic = args.iter().any(|a| a == "geodesic");
    let art_path = args.iter().find(|a| a.ends_with(".txt")).cloned();
    let started = Instant::now();
    let gold = Color::Rgb {
        r: 232,
        g: 180,
        b: 85,
    };

    // Clear to a fresh screen and print the tool banner.
    let _ = execute!(
        io::stdout(),
        Clear(ClearType::All),
        MoveTo(0, 0),
        Print("\r\n  "),
        SetForegroundColor(gold),
        Print("dragonctl"),
        ResetColor,
        Print("  v0.2.0\r\n\r\n")
    );

    step("\u{2713}", Color::Green, "Initializing demo environment");
    pause(450);
    step("\u{2713}", Color::Green, "Updating base image index");
    pause(500);
    step("\u{2193}", Color::Cyan, "Downloading base image");
    let _ = execute!(io::stdout(), Print("\r\n"));

    // The download itself, paced unevenly: a connect stall, a hiccup, a burst, a
    // mid stall, and a slow tail.
    let total: u64 = 48 * 1024 * 1024;
    let mb = 1024.0 * 1024.0;
    let schedule: &[(f64, u64)] = &[
        (0.0, 600),
        (5.0, 300),
        (8.0, 250),
        (8.0, 650),
        (16.0, 220),
        (27.0, 180),
        (38.0, 200),
        (45.0, 450),
        (56.0, 200),
        (67.0, 220),
        (78.0, 260),
        (87.0, 320),
        (93.0, 480),
        (97.0, 520),
        (100.0, 300),
    ];

    let style = if rainbow {
        Style::rainbow()
    } else {
        Style::default()
    };
    let mut builder = Loader::builder().total(total).style(style);
    if geodesic {
        builder = builder.ordering(Geodesic::default());
    }
    if let Some(path) = &art_path {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("could not read {path}: {e}");
            std::process::exit(1);
        });
        builder = builder.art(inkling::Art::parse(&text));
    }
    let loader = builder.start();
    for &(percent, dwell) in schedule {
        let done = (percent / 100.0 * total as f64) as u64;
        loader.set(done);
        loader.set_message(format!(
            "dragon.iso   {:.1} / {:.1} MB",
            done as f64 / mb,
            total as f64 / mb
        ));
        pause(dwell);
    }
    loader.finish();

    // A finish that ties it together.
    let _ = execute!(io::stdout(), Print("\r\n"));
    step(
        "\u{2713}",
        Color::Green,
        "Verified checksum  sha256:a1b2c3d4e5",
    );
    pause(400);
    let _ = execute!(
        io::stdout(),
        Print("\r\n  "),
        SetForegroundColor(Color::Green),
        Print("Done"),
        ResetColor,
        Print(format!(
            " in {:.1}s. dragon.iso (48.0 MB) ready.\r\n\r\n",
            started.elapsed().as_secs_f64()
        ))
    );
    let _ = io::stdout().flush();
}
