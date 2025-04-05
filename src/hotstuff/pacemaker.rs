use std::time::{Duration, Instant};

use crate::config;

use super::replica::{self, ViewNumber};

pub struct Pacemaker {
    pub curr_view: ViewNumber,
    pub timeout: Duration,
    pub last_view_change: Instant,
    replica_ids: Vec<usize>,
    // pub highest_qc: QuorumCertificate,
    // pub leaf: Block,
}

impl Pacemaker {
    pub(crate) fn new() -> Self {
        //   let genesis_block = Block::create_genesis_block();

        // let justify = match &genesis_block {
        //   Block::Genesis { justify, .. } => justify.clone(),
        // _ => panic!("Expected Genesis block, got something else"),
        // };

        let replica_len = config::retrieve_num_validators();
        let replica_ids = (0..replica_len).collect();
        Self {
            curr_view: 0,
            timeout: config::retrieve_tick_duration(),
            last_view_change: Instant::now(),
            replica_ids,
            //   highest_qc: justify,
            //   leaf: genesis_block,
        }
    }

    pub(crate) fn should_advance_view(&self) -> bool {
        self.last_view_change.elapsed() > self.timeout
    }

    pub(crate) fn advance_view(&mut self) {
        println!("advancing view");
        self.curr_view += 1;
        self.last_view_change = Instant::now();
    }

    pub(crate) fn set_view(&mut self, view_number: ViewNumber) {
        self.curr_view = view_number;
        self.last_view_change = Instant::now();
    }

    pub(crate) fn current_leader(&self) -> usize {
        self.replica_ids[(self.curr_view as usize) % self.replica_ids.len()]
    }

    pub(crate) fn time_remaining(&self) -> Duration {
        let end_time = self.last_view_change + self.timeout;
        end_time.saturating_duration_since(Instant::now())
    }
}
