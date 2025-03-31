use std::time::{Duration, Instant};

use super::config;

use super::replica::ViewNumber;

pub struct Pacemaker {
    pub curr_view: ViewNumber,
    pub timeout: Duration,
    pub last_view_change: Instant,
}

impl Pacemaker {
    pub fn new() -> Self {
        Self {
            curr_view: 0,
            timeout: config::retrieve_tick_duration(),
            last_view_change: Instant::now(),
        }
    }

    pub fn should_advance_view(&self) -> bool {
        self.last_view_change.elapsed() > self.timeout
    }

    pub fn advance_view(&mut self) {
        println!("advancing view");
        self.curr_view += 1;
        self.last_view_change = Instant::now();
    }

    pub fn set_view(&mut self, view_number: ViewNumber) {
        self.curr_view = view_number;
        self.last_view_change = Instant::now();
    }

    pub fn current_leader(&self, replica_ids: &[usize]) -> usize {
        replica_ids[(self.curr_view as usize) % replica_ids.len()]
    }

    pub fn time_remaining(&self) -> Duration {
        let end_time = self.last_view_change + self.timeout;
        end_time.saturating_duration_since(Instant::now())
    }
}
