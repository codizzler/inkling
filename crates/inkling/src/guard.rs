//! Durable terminal restoration.
//!
//! `Drop` is enough for the happy path and for panics that unwind, but neither
//! covers Ctrl+C: `SIGINT` terminates the process without unwinding, so a reveal
//! that hid the cursor leaves the user staring at an invisible caret in their
//! shell until they run `reset`. This module makes restoration durable:
//!
//! * a **signal handler** (`SIGINT`/`SIGTERM`/`SIGHUP` on Unix, the console
//!   control handler on Windows) restores the terminal, then lets the default
//!   disposition kill the process, so `^C` still exits 130;
//! * a **panic hook** restores before the message is printed, so a backtrace
//!   never lands in the middle of the art;
//! * `Drop` continues to handle the ordinary path.
//!
//! Restoration is idempotent, so all three racing is harmless. What gets emitted
//! is tracked by two flags rather than assumed, so a session that only hid the
//! cursor does not also get an alternate-screen exit it never asked for.

// Only the non-unix writers go through `std::io`; the unix one writes to fd 1
// directly, because `write` is async-signal-safe and `println!`-style buffering
// is not.
#[cfg(not(unix))]
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Once;

/// Show the cursor.
const SHOW_CURSOR: &[u8] = b"\x1b[?25h";
/// Leave the alternate screen.
const LEAVE_ALT: &[u8] = b"\x1b[?1049l";
/// Reset colours and attributes.
const RESET_SGR: &[u8] = b"\x1b[0m";
/// End synchronized output. Emitted unconditionally: dying between
/// [`crate::render::SYNC_BEGIN`] and its end would otherwise leave a terminal
/// that honours DEC 2026 buffering forever.
const SYNC_END: &[u8] = b"\x1b[?2026l";

static CURSOR_HIDDEN: AtomicBool = AtomicBool::new(false);
static ALT_SCREEN: AtomicBool = AtomicBool::new(false);
static ARMED: Once = Once::new();

/// Record that the cursor is currently hidden.
pub(crate) fn set_cursor_hidden(hidden: bool) {
    CURSOR_HIDDEN.store(hidden, SeqCst);
}

/// Record that we are currently in the alternate screen.
pub(crate) fn set_alt_screen(active: bool) {
    ALT_SCREEN.store(active, SeqCst);
}

/// Install the signal handler and panic hook, once per process. Called by
/// whichever renderer first takes over the terminal.
pub(crate) fn arm() {
    ARMED.call_once(|| {
        enable_virtual_terminal();
        install_panic_hook();
        install_signal_handler();
    });
}

/// Put the terminal back the way we found it. Idempotent, and safe to call from
/// a signal handler: it only issues a single `write` to the standard output.
pub(crate) fn restore() {
    let mut buf = Vec::with_capacity(32);
    buf.extend_from_slice(SYNC_END);
    buf.extend_from_slice(RESET_SGR);
    if CURSOR_HIDDEN.swap(false, SeqCst) {
        buf.extend_from_slice(SHOW_CURSOR);
    }
    if ALT_SCREEN.swap(false, SeqCst) {
        buf.extend_from_slice(LEAVE_ALT);
    }
    write_stdout(&buf);
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restore first: the default hook writes the message immediately, and it
        // must not land inside the art or inside the alternate screen.
        restore();
        previous(info);
    }));
}

// ---------------------------------------------------------------------------
// Unix
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn write_stdout(buf: &[u8]) {
    // SAFETY: a plain write to fd 1 of a buffer we own. `write` is on the list of
    // async-signal-safe functions, which is what lets `restore` run in a handler.
    unsafe {
        let _ = libc::write(libc::STDOUT_FILENO, buf.as_ptr().cast(), buf.len());
    }
}

#[cfg(unix)]
fn install_signal_handler() {
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: `handler` is async-signal-safe (one `write`, then it restores the
        // default disposition and re-raises so the process dies as it normally would).
        unsafe {
            // Through a pointer rather than straight to an integer: casting a
            // function item to an integer is a lint in recent toolchains.
            libc::signal(signal, handler as *const () as libc::sighandler_t);
        }
    }
}

#[cfg(unix)]
extern "C" fn handler(signal: libc::c_int) {
    restore();
    // Die the way we would have without the handler, so `^C` still exits 130 and
    // a parent shell sees the signal rather than a plain non-zero status.
    // SAFETY: both calls are async-signal-safe.
    unsafe {
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
    }
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn write_stdout(buf: &[u8]) {
    let mut out = std::io::stdout();
    let _ = out.write_all(buf);
    let _ = out.flush();
}

#[cfg(windows)]
mod win {
    // Declared directly rather than pulled from a crate: this is two documented
    // calls into kernel32, which std already links, and the core stays lean.
    pub type Bool = i32;
    pub type Dword = u32;

    pub const CTRL_C_EVENT: Dword = 0;
    pub const CTRL_BREAK_EVENT: Dword = 1;
    pub const CTRL_CLOSE_EVENT: Dword = 2;

    pub const STD_OUTPUT_HANDLE: Dword = -11i32 as Dword;
    pub const ENABLE_VIRTUAL_TERMINAL_PROCESSING: Dword = 0x0004;

    pub type Handle = *mut core::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(Dword) -> Bool>,
            add: Bool,
        ) -> Bool;
        pub fn GetStdHandle(which: Dword) -> Handle;
        pub fn GetConsoleMode(handle: Handle, mode: *mut Dword) -> Bool;
        pub fn SetConsoleMode(handle: Handle, mode: Dword) -> Bool;
    }
}

/// Turn on virtual-terminal processing for this console.
///
/// Every renderer in this crate writes escape sequences directly rather than
/// routing colour through the Win32 console API, which is what keeps them
/// testable against a plain writer. A Windows console starts with
/// `ENABLE_VIRTUAL_TERMINAL_PROCESSING` off, though, so without this the
/// sequences do nothing and the reveal comes out monochrome. Windows 10 1703
/// and later accept it; older builds fail the call and are left as they were.
#[cfg(windows)]
fn enable_virtual_terminal() {
    // SAFETY: the standard output handle is read-only here, and the mode is
    // written back with exactly the bit we need added.
    unsafe {
        let handle = win::GetStdHandle(win::STD_OUTPUT_HANDLE);
        let mut mode: win::Dword = 0;
        if win::GetConsoleMode(handle, &mut mode) != 0 {
            win::SetConsoleMode(handle, mode | win::ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

/// Everywhere else the terminal already speaks escape sequences.
#[cfg(not(windows))]
fn enable_virtual_terminal() {}

#[cfg(windows)]
fn install_signal_handler() {
    // SAFETY: registering a handler with a `'static` function pointer.
    unsafe {
        win::SetConsoleCtrlHandler(Some(handler), 1);
    }
}

#[cfg(windows)]
unsafe extern "system" fn handler(event: win::Dword) -> win::Bool {
    match event {
        win::CTRL_C_EVENT | win::CTRL_BREAK_EVENT | win::CTRL_CLOSE_EVENT => {
            restore();
            // Returning false lets the default handler run and terminate us, which
            // is the behaviour a user pressing Ctrl+C expects.
            0
        }
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Anything else: the panic hook still runs, there is just no signal to catch.
// ---------------------------------------------------------------------------

#[cfg(not(any(unix, windows)))]
fn write_stdout(buf: &[u8]) {
    let mut out = std::io::stdout();
    let _ = out.write_all(buf);
    let _ = out.flush();
}

#[cfg(not(any(unix, windows)))]
fn install_signal_handler() {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Restoration must be idempotent: the signal handler, the panic hook, and
    /// `Drop` can all fire, and the second and third must be no-ops.
    #[test]
    fn restore_is_idempotent() {
        set_cursor_hidden(true);
        set_alt_screen(true);
        restore();
        assert!(!CURSOR_HIDDEN.load(SeqCst));
        assert!(!ALT_SCREEN.load(SeqCst));
        restore(); // must not panic or re-emit
        restore();
    }

    #[test]
    fn arming_twice_is_harmless() {
        arm();
        arm();
    }
}
