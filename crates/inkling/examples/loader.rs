//! A tour of Inkling as a loader.
//!
//!   cargo run --example loader            # determinate dragon loader
//!   cargo run --example loader -- iter    # wrap an iterator
//!   cargo run --example loader -- spinner # indeterminate spinner
//!   cargo run --example loader -- threads # progress from worker threads
//!   cargo run --example loader -- rainbow # lolcat-style rainbow palette
//!
//! On a real terminal the dragon paints itself as the work runs. Piped or in CI it
//! prints the finished art once instead.

use std::thread;
use std::time::Duration;

use inkling::{Loader, ProgressIteratorExt};

fn work(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("iter") => {
            for _ in (0..100).inkling() {
                work(20);
            }
        }
        Some("spinner") => {
            let loader = Loader::spinner();
            loader.set_message("Doing something mysterious");
            work(2500);
            loader.finish();
        }
        Some("threads") => {
            let loader = Loader::new(120);
            loader.set_message("Four workers, one dragon");
            thread::scope(|s| {
                for _ in 0..4 {
                    let handle = loader.handle();
                    s.spawn(move || {
                        for _ in 0..30 {
                            work(30);
                            handle.inc(1);
                        }
                    });
                }
            });
            loader.finish();
        }
        Some("rainbow") => {
            let loader = Loader::builder()
                .total(100)
                .style(inkling::render::Style::rainbow())
                .message("Tasting the rainbow")
                .start();
            for _ in 0..100 {
                work(20);
                loader.inc(1);
            }
            loader.finish();
        }
        _ => {
            let total: u64 = 100;
            let loader = Loader::new(total);
            loader.set_message("Summoning the dragon");
            for _ in 0..total {
                work(20);
                loader.inc(1);
            }
            loader.finish();
        }
    }
}
