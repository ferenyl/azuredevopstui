use std::collections::HashMap;

use tokio::task::AbortHandle;

/// A request whose response replaces one part of the shown data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Load {
    MyPullRequests,
    OtherPullRequests,
    Builds,
    Mentions,
    Sprint,
    Detail,
    Signals(u32),
}

/// The newest request per [`Load`]; responses from older requests are dropped.
#[derive(Default)]
pub struct Loads {
    last_generation: u64,
    in_flight: HashMap<Load, (u64, AbortHandle)>,
}

impl Loads {
    pub fn is_loading(&self, load: Load) -> bool {
        self.in_flight.contains_key(&load)
    }

    #[cfg(test)]
    pub fn generation(&self, load: Load) -> Option<u64> {
        self.in_flight.get(&load).map(|(generation, _)| *generation)
    }

    /// Supersedes a request already in flight for `load`; returns the generation of the next one.
    pub fn next(&mut self, load: Load) -> u64 {
        self.cancel(load);
        self.last_generation += 1;
        self.last_generation
    }

    pub fn track(&mut self, load: Load, generation: u64, abort: AbortHandle) {
        self.in_flight.insert(load, (generation, abort));
    }

    /// True when the response is from the newest request for `load`, which is then done.
    pub fn finish(&mut self, load: Load, generation: u64) -> bool {
        let current = self
            .in_flight
            .get(&load)
            .is_some_and(|(newest, _)| *newest == generation);
        if current {
            self.in_flight.remove(&load);
        }
        current
    }

    pub fn cancel(&mut self, load: Load) {
        if let Some((_, abort)) = self.in_flight.remove(&load) {
            abort.abort();
        }
    }

    /// Drops every request in flight, e.g. when the project changes.
    pub fn cancel_all(&mut self) {
        for (_, (_, abort)) in self.in_flight.drain() {
            abort.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending() -> AbortHandle {
        tokio::spawn(std::future::pending::<()>()).abort_handle()
    }

    #[tokio::test]
    async fn only_the_newest_request_finishes() {
        let mut loads = Loads::default();
        let old = loads.next(Load::Sprint);
        let old_abort = pending();
        loads.track(Load::Sprint, old, old_abort.clone());
        let new = loads.next(Load::Sprint);
        loads.track(Load::Sprint, new, pending());

        assert!(!loads.finish(Load::Sprint, old));
        assert!(loads.is_loading(Load::Sprint));
        assert!(loads.finish(Load::Sprint, new));
        assert!(!loads.is_loading(Load::Sprint));
        tokio::task::yield_now().await;
        assert!(old_abort.is_finished());
    }

    #[tokio::test]
    async fn cancelled_requests_do_not_finish() {
        let mut loads = Loads::default();
        let builds = loads.next(Load::Builds);
        loads.track(Load::Builds, builds, pending());
        let signals = loads.next(Load::Signals(1));
        loads.track(Load::Signals(1), signals, pending());

        loads.cancel_all();

        assert!(!loads.finish(Load::Builds, builds));
        assert!(!loads.finish(Load::Signals(1), signals));
    }
}
