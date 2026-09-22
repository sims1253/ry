//! Refresh claims and reconciliation duties, accessed under the backend state lock.
//!
//! Claims order refreshes per path. Completion can retire only duties covered by
//! its epoch; epochs assume no full u64 wrap within an outstanding obligation.
//! Installed bytes still owe context refresh and publication. Cancellation leaves
//! in-flight counts intact so an old exit cannot make a newer refresh look idle.
//! Progress counts retirements, not map size. Stalled rounds retain all work and
//! back off from 40 to 320 ms after eight rounds without a retirement.

use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RefreshDuty {
    BytesContextPublication,
    ContextPublication,
}

#[derive(Clone, Copy)]
struct PendingRefresh {
    epoch: u64,
    duty: RefreshDuty,
}

pub(super) const RECONCILE_ROUNDS_BEFORE_PACING: u32 = 8;
const RECONCILE_PACE_BASE_MILLIS: u64 = 40;
const RECONCILE_PACE_CAP_MILLIS: u64 = 320;

fn reconcile_pace_delay(rounds_without_progress: u32) -> std::time::Duration {
    const RECONCILE_PACE_LADDER_DEPTH: u32 = 3;
    const _: () = assert!(
        RECONCILE_PACE_BASE_MILLIS << RECONCILE_PACE_LADDER_DEPTH == RECONCILE_PACE_CAP_MILLIS
    );
    let doublings = rounds_without_progress
        .saturating_sub(RECONCILE_ROUNDS_BEFORE_PACING)
        // The base already sits at rung 0; the ladder's depth reaches
        // the cap, and every later paced round stays there.
        .min(RECONCILE_PACE_LADDER_DEPTH);
    let millis = (RECONCILE_PACE_BASE_MILLIS << doublings).min(RECONCILE_PACE_CAP_MILLIS);
    std::time::Duration::from_millis(millis)
}

pub(super) enum ReconcileRoundProgress {
    Progress,
    Stalled,
    Pacing {
        delay: std::time::Duration,
        warn: bool,
    },
}

#[derive(Default)]
pub(super) struct Reconciliation {
    refresh_epochs: HashMap<String, u64>,
    refresh_epoch_counter: u64,
    pending_refreshes: HashMap<String, PendingRefresh>,
    refreshes_in_flight: HashMap<String, u32>,
    retired_refreshes: u64,
    reconcile_driver_active: bool,
    reconcile_rounds_without_progress: u32,
    shutting_down: bool,
}

impl Reconciliation {
    pub(super) fn settle_pending_refresh(&mut self, path: &str, epoch: u64, landed: bool) {
        let Some(pending) = self.pending_refreshes.get_mut(path) else {
            return;
        };
        if pending.epoch > epoch {
            return;
        }
        if landed {
            pending.duty = RefreshDuty::ContextPublication;
        } else {
            self.pending_refreshes.remove(path);
            self.retired_refreshes = self.retired_refreshes.wrapping_add(1);
        }
    }

    pub(super) fn claim_refresh_epoch(&mut self, path: &str) -> Option<u64> {
        if self.shutting_down {
            return None;
        }
        self.refresh_epoch_counter = self.refresh_epoch_counter.wrapping_add(1);
        let refresh_epoch = self.refresh_epoch_counter;
        self.refresh_epochs.insert(path.to_string(), refresh_epoch);
        *self
            .refreshes_in_flight
            .entry(path.to_string())
            .or_insert(0) += 1;
        let pending = self
            .pending_refreshes
            .entry(path.to_string())
            .or_insert(PendingRefresh {
                epoch: refresh_epoch,
                duty: RefreshDuty::BytesContextPublication,
            });
        pending.epoch = refresh_epoch;
        pending.duty = RefreshDuty::BytesContextPublication;
        Some(refresh_epoch)
    }

    pub(super) fn note_refresh_exit(&mut self, path: &str) -> bool {
        if let Some(in_flight) = self.refreshes_in_flight.get_mut(path) {
            *in_flight = in_flight.saturating_sub(1);
            if *in_flight == 0 {
                self.refreshes_in_flight.remove(path);
            }
        }
        self.pending_refreshes.contains_key(path) && !self.refreshes_in_flight.contains_key(path)
    }

    fn has_drivable_obligation(&self) -> bool {
        !self.shutting_down
            && self
                .pending_refreshes
                .iter()
                .any(|(path, _)| !self.refreshes_in_flight.contains_key(path))
    }

    pub(super) fn reconcile_round_progress(
        &mut self,
        retired_before: u64,
    ) -> ReconcileRoundProgress {
        if self.retired_refreshes != retired_before {
            self.reconcile_rounds_without_progress = 0;
            return ReconcileRoundProgress::Progress;
        }
        if self.pending_refreshes.is_empty() {
            return ReconcileRoundProgress::Progress;
        }
        self.reconcile_rounds_without_progress =
            self.reconcile_rounds_without_progress.saturating_add(1);
        if self.reconcile_rounds_without_progress < RECONCILE_ROUNDS_BEFORE_PACING {
            return ReconcileRoundProgress::Stalled;
        }
        ReconcileRoundProgress::Pacing {
            delay: reconcile_pace_delay(self.reconcile_rounds_without_progress),
            warn: self.reconcile_rounds_without_progress == RECONCILE_ROUNDS_BEFORE_PACING,
        }
    }

    pub(super) fn complete_pending_publication(&mut self, path: &str, epoch: u64) {
        if self.pending_refreshes.get(path).is_some_and(|pending| {
            pending.duty == RefreshDuty::ContextPublication && pending.epoch <= epoch
        }) {
            self.pending_refreshes.remove(path);
            self.retired_refreshes = self.retired_refreshes.wrapping_add(1);
        }
    }

    pub(super) fn shutdown(&mut self) {
        self.shutting_down = true;
        self.pending_refreshes.clear();
    }

    pub(super) fn is_shutting_down(&self) -> bool {
        self.shutting_down
    }

    pub(super) fn cancel_where(&mut self, removed: impl Fn(&str) -> bool) {
        self.pending_refreshes.retain(|path, _| !removed(path));
    }

    pub(super) fn owns_epoch(&self, path: &str, epoch: u64) -> bool {
        self.refresh_epochs.get(path) == Some(&epoch)
    }

    pub(super) fn forget_epoch(&mut self, path: &str) {
        self.refresh_epochs.remove(path);
    }

    pub(super) fn start_driver(&mut self) -> bool {
        if self.reconcile_driver_active || self.shutting_down || self.pending_refreshes.is_empty() {
            return false;
        }
        self.reconcile_driver_active = true;
        true
    }

    fn idle(&mut self) {
        self.reconcile_driver_active = false;
        self.reconcile_rounds_without_progress = 0;
    }

    pub(super) fn begin_round(&mut self) -> Option<(Vec<String>, u64)> {
        if self.pending_refreshes.is_empty() || self.shutting_down {
            self.idle();
            return None;
        }
        Some((self.pending_paths(), self.retired_refreshes))
    }

    pub(super) fn pending_paths(&self) -> Vec<String> {
        self.pending_refreshes.keys().cloned().collect()
    }

    pub(super) fn drivable_duty(&self, path: &str) -> Option<(RefreshDuty, u64)> {
        if self.shutting_down || self.refreshes_in_flight.contains_key(path) {
            return None;
        }
        self.pending_refreshes
            .get(path)
            .map(|pending| (pending.duty, pending.epoch))
    }

    /// Check for work and clear the active flag under the same state lock.
    pub(super) fn idle_if_blocked(&mut self) -> bool {
        if self.has_drivable_obligation() {
            return false;
        }
        self.idle();
        true
    }
}

#[cfg(test)]
mod reconcile_accounting_tests {
    use super::*;
    #[test]
    fn pacing_counts_retirements_not_map_shrinkage() {
        let mut state = Reconciliation::default();
        let epoch = state.claim_refresh_epoch("/a.R").unwrap();
        assert!(state.note_refresh_exit("/a.R"));

        // Eight rounds that each retire one obligation while one
        // arrival lands mid-round: the map size is constant throughout,
        // so a length comparison would count every round as stalled.
        let mut current = "/a.R";
        for round in 0..8 {
            let retired_before = state.retired_refreshes;
            // The round drives `current` to a terminal outcome
            // (settling removes — retires — the obligation)...
            state.settle_pending_refresh(current, epoch.wrapping_add(round), false);
            // ...while an event arrives for a fresh path, claimed and
            // still in flight when the round ends.
            let arrival = format!("/arrival{round}.R");
            state.claim_refresh_epoch(&arrival).unwrap();
            assert_eq!(
                state.retired_refreshes,
                retired_before + 1,
                "round {round} must retire exactly its driven obligation"
            );
            assert!(
                matches!(
                    state.reconcile_round_progress(retired_before),
                    ReconcileRoundProgress::Progress
                ),
                "a round that retired an obligation is progress; arrivals cannot mask it"
            );
            assert_eq!(
                state.reconcile_rounds_without_progress, 0,
                "retiring rounds must reset the stall count"
            );
            current = Box::leak(arrival.into_boxed_str());
        }

        // Retirement-free rounds over the survivor: the first seven
        // accumulate, the eighth ENTERS the stall episode (warning,
        // base delay) and every later one keeps pacing at a grown,
        // capped delay — but the obligation is retained throughout,
        // which is the whole contract: a finite stall streak must never
        // cancel the work it owes.
        for stalled in 1..=11u32 {
            let retired_before = state.retired_refreshes;
            let progress = state.reconcile_round_progress(retired_before);
            assert_eq!(state.retired_refreshes, retired_before);
            if stalled < RECONCILE_ROUNDS_BEFORE_PACING {
                assert!(
                    matches!(progress, ReconcileRoundProgress::Stalled),
                    "round {stalled} is below the bound"
                );
                assert_eq!(state.reconcile_rounds_without_progress, stalled);
            } else {
                let ReconcileRoundProgress::Pacing { delay, warn } = progress else {
                    panic!("round {stalled} must pace, not drop");
                };
                assert_eq!(delay, reconcile_pace_delay(stalled));
                assert_eq!(
                    warn,
                    stalled == RECONCILE_ROUNDS_BEFORE_PACING,
                    "the warning fires exactly once per stall episode"
                );
                assert!(
                    state.pending_refreshes.contains_key(current),
                    "a paced round retains the obligation"
                );
            }
        }
        assert_eq!(
            state.reconcile_rounds_without_progress, 11,
            "paced rounds keep accumulating until a retirement resets them"
        );

        // A retirement ends the episode: the next retirement-free
        // streak starts a fresh one that warns again at its own
        // crossing, from the base delay.
        state.settle_pending_refresh(current, u64::MAX, false);
        assert!(matches!(
            state.reconcile_round_progress(state.retired_refreshes.wrapping_sub(1)),
            ReconcileRoundProgress::Progress
        ));
        assert_eq!(state.reconcile_rounds_without_progress, 0);
        state.claim_refresh_epoch("/next.R").unwrap();
        for _ in 0..RECONCILE_ROUNDS_BEFORE_PACING - 1 {
            let _ = state.reconcile_round_progress(state.retired_refreshes);
        }
        let ReconcileRoundProgress::Pacing { delay, warn } =
            state.reconcile_round_progress(state.retired_refreshes)
        else {
            panic!("the fresh streak must reach the pacing bound");
        };
        assert!(warn, "a new stall episode warns again");
        assert_eq!(delay, reconcile_pace_delay(RECONCILE_ROUNDS_BEFORE_PACING));
        assert!(state.pending_refreshes.contains_key("/next.R"));
    }

    #[test]
    fn pace_delay_ladder_is_bounded() {
        assert_eq!(
            reconcile_pace_delay(RECONCILE_ROUNDS_BEFORE_PACING),
            std::time::Duration::from_millis(RECONCILE_PACE_BASE_MILLIS)
        );
        assert_eq!(
            reconcile_pace_delay(RECONCILE_ROUNDS_BEFORE_PACING + 1),
            std::time::Duration::from_millis(RECONCILE_PACE_BASE_MILLIS * 2)
        );
        assert_eq!(
            reconcile_pace_delay(RECONCILE_ROUNDS_BEFORE_PACING + 2),
            std::time::Duration::from_millis(RECONCILE_PACE_BASE_MILLIS * 4)
        );
        for stalled in RECONCILE_ROUNDS_BEFORE_PACING..(RECONCILE_ROUNDS_BEFORE_PACING + 50) {
            assert!(
                reconcile_pace_delay(stalled)
                    <= std::time::Duration::from_millis(RECONCILE_PACE_CAP_MILLIS)
            );
        }
        // Below the bound the driver does not sleep at all; rung 0
        // still computes the base delay, pinning the ladder's floor.
        assert_eq!(
            reconcile_pace_delay(0),
            std::time::Duration::from_millis(RECONCILE_PACE_BASE_MILLIS)
        );
    }

    #[test]
    fn older_completion_cannot_retire_a_newer_pending_entry() {
        let mut state = Reconciliation::default();
        // Refresh A claims, exits, lands its bytes, and settles its
        // context — everything but the final acknowledgement.
        let epoch_a = state.claim_refresh_epoch("/a.R").unwrap();
        assert!(state.note_refresh_exit("/a.R"));
        state.settle_pending_refresh("/a.R", epoch_a, true);
        // Before A acknowledges, refresh B claims (re-arming the entry
        // at a higher epoch with a FULL duty) and lands ITS bytes: the
        // entry advances to the context/publication phase at B's
        // epoch — the exact shape a duty-only acknowledgement would
        // mistake for A's completed phase.
        let epoch_b = state.claim_refresh_epoch("/a.R").unwrap();
        assert!(epoch_b > epoch_a);
        assert!(state.note_refresh_exit("/a.R"));
        state.settle_pending_refresh("/a.R", epoch_b, true);

        let retired_before = state.retired_refreshes;
        state.complete_pending_publication("/a.R", epoch_a);
        assert_eq!(
            state.retired_refreshes, retired_before,
            "A's older acknowledgement must retire nothing"
        );
        let pending = state
            .pending_refreshes
            .get("/a.R")
            .expect("B's entry must survive A's acknowledgement");
        assert_eq!(pending.epoch, epoch_b);
        assert_eq!(pending.duty, RefreshDuty::ContextPublication);

        // B's own completion retires it.
        state.complete_pending_publication("/a.R", epoch_b);
        assert_eq!(state.retired_refreshes, retired_before + 1);
        assert!(!state.pending_refreshes.contains_key("/a.R"));
        // A late replay of the older acknowledgement is a no-op.
        state.complete_pending_publication("/a.R", epoch_a);
        assert_eq!(state.retired_refreshes, retired_before + 1);
    }

    #[test]
    fn completion_token_covers_older_entries_but_not_full_duties() {
        let mut state = Reconciliation::default();
        let epoch_a = state.claim_refresh_epoch("/a.R").unwrap();
        state.note_refresh_exit("/a.R");
        state.settle_pending_refresh("/a.R", epoch_a, true);
        // A newer refresh (epoch_c > epoch_a) completes the
        // context/publication phase without re-claiming in between: its
        // acknowledgement covers A's revision.
        let epoch_c = epoch_a + 7;
        state.complete_pending_publication("/a.R", epoch_c);
        assert!(!state.pending_refreshes.contains_key("/a.R"));
        assert_eq!(state.retired_refreshes, 1);

        // An event re-arms the entry with a full duty at a fresh
        // epoch; no acknowledgement, however new, may retire a FULL
        // duty by phase name alone.
        let epoch_d = state.claim_refresh_epoch("/a.R").unwrap();
        state.note_refresh_exit("/a.R");
        state.complete_pending_publication("/a.R", epoch_d + 100);
        let pending = state
            .pending_refreshes
            .get("/a.R")
            .expect("a full duty survives any publication acknowledgement");
        assert_eq!(pending.epoch, epoch_d);
        assert_eq!(pending.duty, RefreshDuty::BytesContextPublication);
        assert_eq!(state.retired_refreshes, 1);
    }

    #[test]
    fn in_flight_accounting_survives_obligation_cancellation() {
        let mut state = Reconciliation::default();
        assert!(state.claim_refresh_epoch("/a.R").is_some());
        // The cancellation drops the obligation (root-removal retain,
        // shutdown clear) but cannot recall the refresh.
        state.cancel_where(|_| true);
        // A later event re-enqueues before the old refresh returns.
        let rearm_epoch = state.claim_refresh_epoch("/a.R").unwrap();
        assert_eq!(
            state.refresh_epochs.get("/a.R"),
            Some(&rearm_epoch),
            "the fresh claim owns the path"
        );
        assert!(
            !state.note_refresh_exit("/a.R"),
            "the old refresh's exit must not report the new obligation drivable"
        );
        assert!(
            !state.has_drivable_obligation(),
            "the driver must not preempt the re-armed refresh that is still in flight"
        );
        assert!(
            state.note_refresh_exit("/a.R"),
            "the owning refresh's exit is what makes the obligation drivable"
        );
        assert!(state.has_drivable_obligation());
    }

    #[test]
    fn shutdown_refuses_refresh_claims() {
        let mut state = Reconciliation::default();
        assert!(state.claim_refresh_epoch("/a.R").is_some());
        state.shutdown();
        assert!(state.claim_refresh_epoch("/b.R").is_none());
        assert!(state.pending_refreshes.is_empty());
        assert!(!state.has_drivable_obligation());
    }
}
