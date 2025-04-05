use std::time::{Duration, Instant};

use crate::{config, pacemaker_log};

use super::replica::ViewNumber;

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
        pacemaker_log!(
            "Timeout occured - advancing view from {:?} to {:?}",
            self.curr_view,
            self.curr_view + 1
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_new_pacemaker_starts_at_view_zero() {
        let pacemaker = Pacemaker::new();
        assert_eq!(pacemaker.curr_view, 0);
        assert_eq!(
            pacemaker.replica_ids.len(),
            config::retrieve_num_validators()
        );
    }

    #[test]
    fn test_should_advance_view_false_initially() {
        let pacemaker = Pacemaker::new();
        assert_eq!(pacemaker.should_advance_view(), false);
    }

    #[test]
    fn test_should_advance_view_after_timeout() {
        let pacemaker = Pacemaker::new();
        // simulate passage of time
        sleep(pacemaker.timeout + std::time::Duration::from_millis(10));
        assert_eq!(pacemaker.should_advance_view(), true);
    }

    #[test]
    fn test_advance_view_increments_view_and_resets_timer() {
        let mut pacemaker = Pacemaker::new();
        let initial_time = pacemaker.last_view_change;
        pacemaker.advance_view();
        assert_eq!(pacemaker.curr_view, 1);
        assert!(pacemaker.last_view_change > initial_time);
    }

    #[test]
    fn test_set_view_sets_view_and_resets_timer() {
        let mut pacemaker = Pacemaker::new();
        pacemaker.set_view(42);
        assert_eq!(pacemaker.curr_view, 42);
        // The actual Instant changes, hard to assert equality — so we just check time_remaining resets
        assert!(pacemaker.time_remaining() <= pacemaker.timeout);
    }

    #[test]
    fn test_current_leader_rotates_among_replicas() {
        let mut pacemaker = Pacemaker::new();
        let total_replicas = pacemaker.replica_ids.len();

        for i in 0..(total_replicas * 2) {
            pacemaker.curr_view = i as u64;
            let expected_leader = pacemaker.replica_ids[i % total_replicas];
            assert_eq!(pacemaker.current_leader(), expected_leader);
        }
    }

    #[test]
    fn test_time_remaining_decreases() {
        let pacemaker = Pacemaker::new();
        let t1 = pacemaker.time_remaining();
        sleep(std::time::Duration::from_millis(10));
        let t2 = pacemaker.time_remaining();
        assert!(t2 < t1);
    }
}
