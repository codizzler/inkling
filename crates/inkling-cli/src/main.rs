//! `inkling`: pipe progress in, watch ASCII art reveal.
//!
//! This is the language-agnostic bridge. Anything that can write to a pipe, a
//! bash script, Python, Node, a Makefile, can drive the reveal the way you would
//! pipe to `pv`, with no bindings to link against.
//!
//! Reads progress from stdin, one token per line:
//!
//! ```text
//! N        set absolute progress to N
//! +N       advance progress by N
//! <text>   any non-numeric line becomes the caption
//! ```
//!
//! On end of input the art finishes filled.
//!
//! ```sh
//! seq 0 100 | inkling --total 100
//! inkling --total 100 --rainbow --art snake.txt < progress.log
//! ```

use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;

use inkling::easing::Easing;
use inkling::ordering::{Directional, Geodesic, Scanline, StartHint};
use inkling::render::{ColorDepth, Palette, Style};
use inkling::{Art, Loader};

/// The installed binary is `inkling`; the crate is `inkling-cli`. Diagnostics
/// should name what the user typed.
const NAME: &str = env!("CARGO_BIN_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
inkling: reveal ASCII art from progress on stdin

USAGE:
    <producer> | inkling [OPTIONS]

OPTIONS:
    -t, --total <N>      total units of work; omit for an indeterminate spinner
    -a, --art <FILE>     ASCII art to reveal (default: the built-in dragon)
    -m, --message <MSG>  initial caption shown beneath the art

REVEAL:
        --geodesic       trace the art's spine instead of a directional wipe
        --scanline       plain reading order, the baseline
        --reading        wipe along the locale's reading direction
        --ltr, --rtl     wipe left-to-right or right-to-left explicitly
        --start <WHERE>  geodesic start tip: top-left (default), bottom, topological
        --bridge <N>     blank cells the spine may step across (default 1, 0 off)
        --easing <CURVE> linear (default), ease-out-cubic, ease-out-quint,
                         ease-in-out-cubic

COLOUR:
        --rainbow        lolcat-style rainbow palette
        --light          palette tuned for a light terminal background
        --no-color       no colour at all (also honours NO_COLOR)
        --color <DEPTH>  auto (default), truecolor, 256, 16, none
        --head <HEX>     colour at the leading edge, e.g. ffe28a
        --body <HEX>     colour of settled ink, e.g. 7886a8
        --feather <F>    width of the glow, in rank units (default 0.07; 0 off)

    -h, --help           print this help
    -V, --version        print the version

STDIN PROTOCOL (one token per line):
    N        set absolute progress to N
    +N       advance progress by N
    <text>   any non-numeric line becomes the caption

EXAMPLES:
    seq 0 100 | inkling --total 100
    inkling --total 100 --rainbow --art snake.txt < progress.log
    make 2>&1 | inkling --geodesic --easing ease-out-cubic
";

/// Exit code for a usage error, matching the convention `getopt` set.
const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(Fail::Usage(msg)) => {
            eprintln!("{NAME}: {msg}");
            eprintln!("try '{NAME} --help' for the full list of options");
            ExitCode::from(USAGE_ERROR)
        }
        Err(Fail::Io(msg)) => {
            eprintln!("{NAME}: {msg}");
            ExitCode::FAILURE
        }
    }
}

enum Fail {
    /// The command line did not make sense.
    Usage(String),
    /// The command line was fine but something went wrong carrying it out.
    Io(String),
}

/// Every option the command line can set, resolved before anything is built so a
/// bad flag fails before the terminal has been touched.
struct Options {
    total: Option<u64>,
    art: Option<String>,
    message: String,
    ordering: OrderingChoice,
    easing: Easing,
    style: Style,
    start: StartHint,
    bridge: u16,
}

enum OrderingChoice {
    Auto,
    Geodesic,
    Scanline,
    Reading,
    Ltr,
    Rtl,
}

fn run() -> Result<ExitCode, Fail> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut opts = Options {
        total: None,
        art: None,
        message: String::new(),
        ordering: OrderingChoice::Auto,
        easing: Easing::Linear,
        style: Style::default(),
        start: StartHint::TopLeft,
        bridge: Geodesic::default().bridge,
    };
    // Set only by --rainbow/--light so the two can be combined with an explicit
    // --head/--body without one silently resetting the other.
    let mut palette = None;
    let mut light = false;
    let mut depth: Option<ColorDepth> = None;
    let mut head = None;
    let mut body = None;
    let mut feather = None;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        // Accept both `--flag value` and `--flag=value`. `take` is what makes a
        // missing or malformed value an error rather than a silent default, which
        // is how `--total abc` used to become an indeterminate spinner.
        let mut take = |name: &str| -> Result<String, Fail> {
            if let Some(v) = arg.strip_prefix(&format!("{name}=")) {
                return Ok(v.to_string());
            }
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| Fail::Usage(format!("'{name}' needs a value")))
        };
        // `matches` keeps a long option from swallowing its neighbours: `--totally`
        // must be an unknown argument, not `--total` eating the next word.
        let matches = |long: &str, short: Option<&str>| {
            arg == long || Some(arg) == short || arg.starts_with(&format!("{long}="))
        };

        match arg {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(ExitCode::SUCCESS);
            }
            "-V" | "--version" => {
                println!("{NAME} {VERSION}");
                return Ok(ExitCode::SUCCESS);
            }
            "--rainbow" => palette = Some(Palette::Rainbow),
            "--light" => light = true,
            "--no-color" => depth = Some(ColorDepth::Mono),
            "--geodesic" => opts.ordering = OrderingChoice::Geodesic,
            "--scanline" => opts.ordering = OrderingChoice::Scanline,
            "--reading" => opts.ordering = OrderingChoice::Reading,
            "--ltr" => opts.ordering = OrderingChoice::Ltr,
            "--rtl" => opts.ordering = OrderingChoice::Rtl,

            _ if matches("--total", Some("-t")) => {
                let v = take("--total")?;
                opts.total = Some(parse_num(&v, "--total")?);
            }
            _ if matches("--art", Some("-a")) => {
                opts.art = Some(take("--art")?);
            }
            _ if matches("--message", Some("-m")) => {
                opts.message = take("--message")?;
            }
            _ if matches("--start", None) => {
                opts.start = match take("--start")?.as_str() {
                    "top-left" | "topleft" => StartHint::TopLeft,
                    "bottom" => StartHint::Bottom,
                    "topological" => StartHint::Topological,
                    other => {
                        return Err(Fail::Usage(format!(
                            "unknown --start '{other}' (top-left, bottom, topological)"
                        )))
                    }
                };
            }
            _ if matches("--bridge", None) => {
                let v = take("--bridge")?;
                opts.bridge = parse_num(&v, "--bridge")?;
            }
            _ if matches("--easing", None) => {
                opts.easing = match take("--easing")?.as_str() {
                    "linear" => Easing::Linear,
                    "ease-out-cubic" => Easing::EaseOutCubic,
                    "ease-out-quint" => Easing::EaseOutQuint,
                    "ease-in-out-cubic" => Easing::EaseInOutCubic,
                    other => {
                        return Err(Fail::Usage(format!(
                            "unknown --easing '{other}' (linear, ease-out-cubic, \
                             ease-out-quint, ease-in-out-cubic)"
                        )))
                    }
                };
            }
            _ if matches("--color", None) => {
                depth = Some(match take("--color")?.as_str() {
                    "auto" => ColorDepth::detect(),
                    "truecolor" | "24bit" => ColorDepth::TrueColor,
                    "256" => ColorDepth::Ansi256,
                    "16" => ColorDepth::Ansi16,
                    "none" | "mono" => ColorDepth::Mono,
                    other => {
                        return Err(Fail::Usage(format!(
                            "unknown --color '{other}' (auto, truecolor, 256, 16, none)"
                        )))
                    }
                });
            }
            _ if matches("--head", None) => {
                let v = take("--head")?;
                head = Some(parse_hex(&v, "--head")?);
            }
            _ if matches("--body", None) => {
                let v = take("--body")?;
                body = Some(parse_hex(&v, "--body")?);
            }
            _ if matches("--feather", None) => {
                let v = take("--feather")?;
                let f: f32 = v
                    .parse()
                    .map_err(|_| Fail::Usage(format!("--feather needs a number, got '{v}'")))?;
                if !(0.0..=1.0).contains(&f) {
                    return Err(Fail::Usage(format!("--feather must be in 0..=1, got {f}")));
                }
                feather = Some(f);
            }
            other => return Err(Fail::Usage(format!("unknown argument '{other}'"))),
        }
        i += 1;
    }

    // Assemble the style, letting explicit colours win over the presets.
    opts.style = match (light, palette) {
        (true, Some(Palette::Rainbow)) => Style {
            palette: Palette::Rainbow,
            ..Style::light()
        },
        (true, _) => Style::light(),
        (false, Some(Palette::Rainbow)) => Style::rainbow(),
        (false, _) => Style::default(),
    };
    if let Some(d) = depth {
        opts.style.depth = d;
    }
    if let Some(c) = head {
        opts.style.head = c;
    }
    if let Some(c) = body {
        opts.style.body = c;
    }
    if let Some(f) = feather {
        opts.style.feather = f;
    }

    start(opts)
}

fn start(opts: Options) -> Result<ExitCode, Fail> {
    // Art must be read before the loader takes the terminal, so a missing file is
    // a clean error rather than a flash of half-painted dragon. stdin is not an
    // option here: it is already the progress channel.
    let art = match opts.art.as_deref() {
        None => None,
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| Fail::Io(format!("could not read {path}: {e}")))?;
            Some(Art::parse(&text))
        }
    };
    if art.as_ref().is_some_and(|a| a.is_empty()) {
        return Err(Fail::Io("the art file has no ink in it".into()));
    }

    let geodesic = Geodesic {
        start: opts.start,
        bridge: opts.bridge,
    };
    let mut builder = Loader::builder()
        .style(opts.style)
        .easing(opts.easing)
        .message(opts.message);
    if let Some(t) = opts.total {
        builder = builder.total(t);
    }
    if let Some(a) = art {
        builder = builder.art(a);
    }
    builder = match opts.ordering {
        OrderingChoice::Auto => builder.ordering(Directional::default()),
        OrderingChoice::Geodesic => builder.ordering(geodesic),
        OrderingChoice::Scanline => builder.ordering(Scanline),
        OrderingChoice::Reading => builder.ordering(Directional::reading()),
        OrderingChoice::Ltr => builder.ordering(Directional::ltr()),
        OrderingChoice::Rtl => builder.ordering(Directional::rtl()),
    };

    // Nothing will ever arrive on an interactive stdin, so say so rather than
    // hanging behind a half-painted dragon.
    if io::stdin().is_terminal() {
        eprintln!("{NAME}: reading progress from stdin; pipe something in, or see --help");
    }

    let loader = builder.start();
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // clean end of input
            Ok(_) => {}
            // A real read error is worth reporting; the art still finishes so the
            // terminal is left in a sane state either way.
            Err(e) => {
                loader.finish();
                return Err(Fail::Io(format!("reading stdin: {e}")));
            }
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
    Ok(ExitCode::SUCCESS)
}

fn parse_num<T: std::str::FromStr>(value: &str, flag: &str) -> Result<T, Fail> {
    value
        .parse()
        .map_err(|_| Fail::Usage(format!("{flag} needs a whole number, got '{value}'")))
}

/// Parse `rrggbb`, with or without a leading `#`.
fn parse_hex(value: &str, flag: &str) -> Result<(u8, u8, u8), Fail> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    let bad = || Fail::Usage(format!("{flag} needs a colour like ffe28a, got '{value}'"));
    if hex.len() != 6 {
        return Err(bad());
    }
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).map_err(|_| bad());
    Ok((byte(0)?, byte(2)?, byte(4)?))
}
