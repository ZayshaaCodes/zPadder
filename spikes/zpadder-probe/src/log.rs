use std::fmt::Display;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

/// Prints timestamped lines and copies each one to `probe-<name>.log` as it is written, so the
/// file is complete even when the probe is stopped with Ctrl+C.
#[derive(Debug)]
pub(crate) struct Log {
    start: Instant,
    file: Option<File>,
}

impl Log {
    pub(crate) fn create(name: &str) -> Self {
        let path = format!("probe-{name}.log");
        let file = match File::create(&path) {
            Ok(file) => {
                println!("Writing {path}");
                Some(file)
            }
            Err(e) => {
                eprintln!("Cannot write {path}: {e}");
                None
            }
        };
        Self { start: Instant::now(), file }
    }

    pub(crate) fn line(&mut self, msg: impl Display) {
        let line = format!("[{:>8.3}s] {msg}", self.start.elapsed().as_secs_f64());
        println!("{line}");
        if let Some(file) = &mut self.file {
            let _ = writeln!(file, "{line}");
        }
    }
}
