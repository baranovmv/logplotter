use std::time;
pub mod line_reader;

pub struct Once {
    t0: time::Instant,
    period: time::Duration,
}

impl Once {
    pub fn new(period: time::Duration) -> Once {
        Once {
            t0: time::Instant::now(),
            period,
        }
    }

    pub fn once(&mut self) -> bool {
        if self.t0.elapsed() > self.period {
            self.t0 = time::Instant::now();
            true
        } else {
            false
        }

    }
}