use std::fmt::Display;
use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn colors_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stdout().is_terminal()
}

pub fn animations_enabled() -> bool {
    colors_enabled()
}

fn paint(code: &str, body: &str) -> String {
    if colors_enabled() {
        format!("\x1b[{code}m{body}\x1b[0m")
    } else {
        body.to_string()
    }
}

pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn red(s: &str) -> String {
    paint("31", s)
}
pub fn yellow(s: &str) -> String {
    paint("33", s)
}
pub fn cyan(s: &str) -> String {
    paint("36", s)
}
pub fn magenta(s: &str) -> String {
    paint("35", s)
}
pub fn dim(s: &str) -> String {
    paint("90", s)
}
pub fn bold(s: &str) -> String {
    paint("1", s)
}

pub fn ok(msg: impl Display) {
    println!("{} {msg}", green("✔"));
}
pub fn fail(msg: impl Display) {
    println!("{} {msg}", red("✗"));
}
pub fn warn(msg: impl Display) {
    println!("{} {msg}", yellow("⚠"));
}
pub fn info(msg: impl Display) {
    println!("{} {msg}", cyan("➜"));
}
pub fn header(msg: impl Display) {
    println!("{}", bold(&msg.to_string()));
}
pub fn eprintln_error(msg: impl Display) {
    eprintln!("{} {msg}", red("error:"));
}

pub struct Spinner {
    flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    enabled: bool,
}

impl Spinner {
    pub fn start(message: impl Into<String>) -> Self {
        let message = message.into();
        let enabled = animations_enabled();
        if !enabled {
            info(&message);
            return Self {
                flag: Arc::new(AtomicBool::new(false)),
                handle: None,
                enabled,
            };
        }
        let flag = Arc::new(AtomicBool::new(true));
        let flag_c = Arc::clone(&flag);
        let handle = thread::spawn(move || {
            let mut stdout = std::io::stdout();
            let _ = write!(stdout, "\x1b[?25l");
            let _ = stdout.flush();
            let mut i = 0_usize;
            while flag_c.load(Ordering::Relaxed) {
                let frame = FRAMES[i % FRAMES.len()];
                let _ = write!(stdout, "\r\x1b[36m{frame}\x1b[0m {message}");
                let _ = stdout.flush();
                thread::sleep(Duration::from_millis(80));
                i = i.wrapping_add(1);
            }
            let _ = write!(stdout, "\r\x1b[K\x1b[?25h");
            let _ = stdout.flush();
        });
        Self {
            flag,
            handle: Some(handle),
            enabled,
        }
    }

    fn stop_internal(&mut self) {
        self.flag.store(false, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    pub fn finish_ok(mut self, msg: impl Display) {
        self.stop_internal();
        ok(msg);
    }

    pub fn finish_fail(mut self, msg: impl Display) {
        self.stop_internal();
        fail(msg);
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.stop_internal();
            if self.enabled {
                let mut stdout = std::io::stdout();
                let _ = write!(stdout, "\r\x1b[K\x1b[?25h");
                let _ = stdout.flush();
            }
        }
    }
}
