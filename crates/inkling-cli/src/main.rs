//! `inkling`: pipe progress in, watch ASCII art reveal.
//!
//! This is the language-agnostic bridge. Anything that can write to a pipe, a
//! bash script, Python, Node, a Makefile, can drive the reveal the way you would
//! pipe to `pv`, with no bindings to link against.
//!
//! Reads progress from stdin, one token per line:
//!   N        set absolute progress to N
//!   +N       advance progress by N
//!   <text>   any non-numeric line becomes the caption
//! On end of input the art finishes filled.
//!
//!   seq 0 100 | inkling --total 100
//!   inkling --total 100 --rainbow --art snake.txt < progress.log

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use inkling::ordering::{Directional, Geodesic};
use inkling::render::Style;
use inkling::{Art, Loader};

const HELP: &str = "\
inkling: reveal ASCII art from progress on stdin

USAGE:
    <producer> | inkling [OPTIONS]

OPTIONS:
    -t, --total <N>      total units of work; omit for an indeterminate spinner
    -a, --art <FILE>     ASCII art to reveal (default: the built-in dragon)
    -m, --message <MSG>  initial caption shown beneath the art
        --rainbow        lolcat-style rainbow palette
        --geodesic       trace the art's spine instead of a directional wipe
        --reading        wipe along the locale's reading direction
    -h, --help           print this help

STDIN PROTOCOL (one token per line):
    N        set absolute progress to N
    +N       advance progress by N
    <text>   any non-numeric line becomes the caption

EXAMPLES:
    seq 0 100 | inkling --total 100
    inkling --total 100 --rainbow --art snake.txt < progress.log
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut total: Option<u64> = None;
    let mut art_path: Option<String> = None;
    let mut message = String::new();
    let mut rainbow = false;
    let mut geodesic = false;
    let mut reading = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "--rainbow" => rainbow = true,
            "--geodesic" => geodesic = true,
            "--reading" => reading = true,
            "-t" | "--total" => {
                i += 1;
                total = args.get(i).and_then(|v| v.parse().ok());
            }
            "-a" | "--art" => {
                i += 1;
                art_path = args.get(i).cloned();
            }
            "-m" | "--message" => {
                i += 1;
                message = args.get(i).cloned().unwrap_or_default();
            }
            _ if arg.starts_with("--total=") => total = arg[8..].parse().ok(),
            _ if arg.starts_with("--art=") => art_path = Some(arg[6..].to_string()),
            _ if arg.starts_with("--message=") => message = arg[10..].to_string(),
            other => {
                eprintln!("inkling: unknown argument '{other}' (try --help)");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    // Assemble the loader from the flags.
    let style = if rainbow {
        Style::rainbow()
    } else {
        Style::default()
    };
    let mut builder = Loader::builder().style(style).message(message);
    if let Some(t) = total {
        builder = builder.total(t);
    }
    if let Some(path) = &art_path {
        match std::fs::read_to_string(path) {
            Ok(text) => builder = builder.art(Art::parse(&text)),
            Err(e) => {
                eprintln!("inkling: could not read {path}: {e}");
                return ExitCode::from(1);
            }
        }
    }
    if geodesic {
        builder = builder.ordering(Geodesic::default());
    } else if reading {
        builder = builder.ordering(Directional::reading());
    }
    let loader = builder.start();

    // Drive it from stdin, one token per line, until the producer closes the pipe.
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break, // end of input
            Ok(_) => {}
        }
        let token = line.trim();
        if token.is_empty() {
            continue;
        }
        if let Some(rest) = token.strip_prefix('+') {
            if let Ok(delta) = rest.trim().parse::<u64>() {
                loader.inc(delta);
                continue;
            }
        }
        match token.parse::<u64>() {
            Ok(pos) => loader.set(pos),
            Err(_) => loader.set_message(token.to_string()),
        }
    }

    loader.finish();
    let _ = io::stdout().flush();
    ExitCode::SUCCESS
}
