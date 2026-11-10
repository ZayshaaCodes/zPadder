use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use crate::log::Log;

pub(crate) fn run() -> ExitCode {
    let mut log = Log::create("timer");
    log.line("Measuring for about 20 s...");

    for target in [Duration::from_micros(500), Duration::from_millis(1), Duration::from_millis(4)] {
        let mut samples: Vec<Duration> = (0..1000)
            .map(|_| {
                let start = Instant::now();
                thread::sleep(target);
                start.elapsed()
            })
            .collect();
        samples.sort();
        let at = |percent: usize| samples[(samples.len() - 1) * percent / 100];
        log.line(format_args!(
            "sleep({target:?}): median {:?}, p90 {:?}, p99 {:?}, max {:?}",
            at(50),
            at(90),
            at(99),
            at(100),
        ));
    }

    // A 20 Hz turbo for 5 s should fire 100 times. Sleeping a fixed period after each press
    // drifts (each sleep overshoots); sleeping until each deadline does not.
    let period = Duration::from_millis(50);
    let presses = 100u32;
    let start = Instant::now();
    for _ in 0..presses {
        thread::sleep(period);
    }
    let naive = start.elapsed();
    let start = Instant::now();
    for i in 1..=presses {
        let deadline = start + period * i;
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
    }
    let scheduled = start.elapsed();
    let expected = period * presses;
    log.line(format_args!(
        "20 Hz turbo, {presses} presses (ideal {expected:?}): fixed sleeps took {naive:?} \
         ({:+.1}%), deadline scheduling took {scheduled:?} ({:+.1}%)",
        drift(naive, expected),
        drift(scheduled, expected),
    ));
    ExitCode::SUCCESS
}

fn drift(actual: Duration, expected: Duration) -> f64 {
    (actual.as_secs_f64() / expected.as_secs_f64() - 1.0) * 100.0
}
