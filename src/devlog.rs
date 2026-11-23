//! A running log of what the game did, for finding bugs and checking animation timing. On in debug
//! builds (to target/dev-log.txt, fresh each launch) and wherever MINEBOUND_TRACE points; off in tests
//! and in release builds without MINEBOUND_TRACE.

use std::cell::RefCell;
use std::io::{LineWriter, Write};
use std::time::Instant;

thread_local! {
    /// opened on the first line; None inside means logging is off
    static OUT: RefCell<Option<Option<LineWriter<std::fs::File>>>> = const { RefCell::new(None) };
    static START: Instant = Instant::now();
}

fn open() -> Option<LineWriter<std::fs::File>> {
    if cfg!(test) {
        return None;
    }
    let path: std::path::PathBuf = std::env::var_os("MINEBOUND_TRACE").map(Into::into).or_else(|| cfg!(debug_assertions).then(|| "target/dev-log.txt".into()))?;
    std::fs::File::create(path).ok().map(LineWriter::new)
}

/// Appends one line, stamped with seconds since launch. The line is only built when logging is on.
pub fn log(line: impl FnOnce() -> String) {
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        if let Some(w) = o.get_or_insert_with(open) {
            let t = START.with(|s| s.elapsed().as_secs_f64());
            let _ = writeln!(w, "{t:9.3} {}", line());
        }
    });
}

/// Panics go to the log too, with where they happened, before the usual message.
pub fn catch_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log(|| format!("PANIC {info}"));
        default(info);
    }));
}
