//! Bounded, order-preserving fan-out of per-page OCR work.
//!
//! Pages are recognized by at most `workers` threads, but their results are handed to the
//! consumer strictly in page order, so the output is identical to a sequential run. A page may
//! only start while it is fewer than `workers` pages ahead of the next page to be consumed, which
//! bounds the number of in-flight plus buffered page results (and so memory) by `workers`.
use ariad_core::limits::Limits;
use std::{
    collections::BTreeMap,
    sync::{Condvar, Mutex, MutexGuard},
    thread,
};
use tokio_util::sync::CancellationToken;

/// Default ceiling when the machine has more cores than this.
const DEFAULT_MAX_WORKERS: usize = 4;

/// Resolve the worker count from the request limits. A per-process memory budget
/// (`max_memory_mb`, set by the cloud/server profile) means the host must not multiply that
/// budget across concurrent engine processes, so such jobs use one worker. Without a memory
/// budget (local CLI and desktop) the count is `min(cores, 4)`. It never exceeds the number of
/// pages and is never less than one.
pub(super) fn resolve_workers(limits: &Limits, cores: usize, pages: usize) -> usize {
    let configured = if limits.max_memory_mb.is_some() {
        1
    } else {
        cores.clamp(1, DEFAULT_MAX_WORKERS)
    };
    configured.min(pages).max(1)
}

/// Worker count for a job of `pages` pages on this machine under `limits`.
pub(super) fn worker_count(limits: &Limits, pages: usize) -> usize {
    resolve_workers(
        limits,
        thread::available_parallelism().map_or(1, |n| n.get()),
        pages,
    )
}

struct State<T, E> {
    next: usize,
    applied: usize,
    done: BTreeMap<usize, Result<T, E>>,
    stopped: bool,
    panicked: bool,
}

struct Shared<T, E> {
    state: Mutex<State<T, E>>,
    wake: Condvar,
}

impl<T, E> Shared<T, E> {
    fn lock(&self) -> MutexGuard<'_, State<T, E>> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn wait<'a>(&self, guard: MutexGuard<'a, State<T, E>>) -> MutexGuard<'a, State<T, E>> {
        self.wake.wait(guard).unwrap_or_else(|p| p.into_inner())
    }
}

struct PanicGuard<'a, T, E>(&'a Shared<T, E>);

impl<T, E> Drop for PanicGuard<'_, T, E> {
    fn drop(&mut self) {
        if thread::panicking() {
            self.0.lock().panicked = true;
            self.0.wake.notify_all();
        }
    }
}

/// Run `work(lane, index)` for `0..count` on `workers` lanes and feed the results to `consume`
/// in index order on the calling thread.
///
/// The first failure in index order (from `work` or `consume`) is returned, which matches what a
/// sequential loop would report. On failure `stop` is cancelled so in-flight engine calls abort,
/// and no further page is started. `lane` is stable per thread, so callers can keep per-lane
/// resources (a workspace, a warning buffer) indexed by it.
pub(super) fn run_ordered<T: Send, E: Send>(
    count: usize,
    workers: usize,
    stop: &CancellationToken,
    work: impl Fn(usize, usize) -> Result<T, E> + Sync,
    mut consume: impl FnMut(usize, T) -> Result<(), E>,
) -> Result<(), E> {
    let workers = workers.clamp(1, count.max(1));
    let shared = Shared {
        state: Mutex::new(State {
            next: 0,
            applied: 0,
            done: BTreeMap::new(),
            stopped: false,
            panicked: false,
        }),
        wake: Condvar::new(),
    };
    thread::scope(|scope| {
        for lane in 0..workers {
            let shared = &shared;
            let work = &work;
            scope.spawn(move || {
                let _guard = PanicGuard(shared);
                loop {
                    let index = {
                        let mut state = shared.lock();
                        loop {
                            if state.stopped || state.next >= count {
                                return;
                            }
                            if state.next < state.applied + workers {
                                state.next += 1;
                                break state.next - 1;
                            }
                            state = shared.wait(state);
                        }
                    };
                    let result = work(lane, index);
                    shared.lock().done.insert(index, result);
                    shared.wake.notify_all();
                }
            });
        }
        let mut outcome = Ok(());
        for index in 0..count {
            let result = {
                let mut state = shared.lock();
                loop {
                    if let Some(result) = state.done.remove(&index) {
                        break Some(result);
                    }
                    if state.panicked {
                        break None;
                    }
                    state = shared.wait(state);
                }
            };
            // `None` means a worker panicked; the scope re-raises it once every thread stopped.
            let step = result.map_or(Err(None), |r| {
                r.and_then(|value| consume(index, value)).map_err(Some)
            });
            match step {
                Ok(()) => {
                    shared.lock().applied += 1;
                    shared.wake.notify_all();
                }
                Err(error) => {
                    if let Some(error) = error {
                        outcome = Err(error);
                    }
                    stop.cancel();
                    shared.lock().stopped = true;
                    shared.wake.notify_all();
                    break;
                }
            }
        }
        outcome
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    #[test]
    fn local_profile_uses_min_of_cores_and_four() {
        let local = Limits::local();
        assert_eq!(local.max_memory_mb, None);
        assert_eq!(resolve_workers(&local, 16, 100), 4);
        assert_eq!(resolve_workers(&local, 2, 100), 2);
        assert_eq!(resolve_workers(&local, 0, 100), 1);
        // Never more workers than pages, and never fewer than one.
        assert_eq!(resolve_workers(&local, 16, 3), 3);
        assert_eq!(resolve_workers(&local, 16, 0), 1);
    }

    #[test]
    fn memory_budgeted_profiles_use_one_worker() {
        let cloud = Limits::cloud();
        assert!(cloud.max_memory_mb.is_some());
        assert_eq!(resolve_workers(&cloud, 16, 100), 1);
        assert_eq!(resolve_workers(&cloud, 2, 100), 1);
        let mut budgeted = Limits::local();
        budgeted.max_memory_mb = Some(512);
        assert_eq!(resolve_workers(&budgeted, 16, 100), 1);
    }

    #[test]
    fn results_are_consumed_in_page_order_even_when_pages_finish_out_of_order() {
        let stop = CancellationToken::new();
        let mut seen = Vec::new();
        run_ordered::<_, ()>(
            24,
            4,
            &stop,
            |_, index| {
                // Early pages are the slowest, so later pages finish first.
                thread::sleep(Duration::from_millis(((24 - index) % 7 * 4) as u64));
                Ok(index * 10)
            },
            |index, value| {
                assert_eq!(value, index * 10);
                seen.push(index);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(seen, (0..24).collect::<Vec<_>>());
        assert!(!stop.is_cancelled());
    }

    #[test]
    fn parallel_result_equals_sequential_result() {
        let run = |workers| {
            let mut out = Vec::new();
            run_ordered::<_, ()>(
                17,
                workers,
                &CancellationToken::new(),
                |_, i| {
                    thread::sleep(Duration::from_millis((i * 5 % 3) as u64));
                    Ok(format!("page-{i}"))
                },
                |_, value| {
                    out.push(value);
                    Ok(())
                },
            )
            .unwrap();
            out
        };
        assert_eq!(run(1), run(4));
    }

    #[test]
    fn concurrency_and_buffered_results_never_exceed_the_bound() {
        const WORKERS: usize = 3;
        let running = AtomicUsize::new(0);
        let peak_running = AtomicUsize::new(0);
        let started = AtomicUsize::new(0);
        let mut peak_ahead = 0;
        run_ordered::<_, ()>(
            30,
            WORKERS,
            &CancellationToken::new(),
            |_, index| {
                started.fetch_add(1, Ordering::SeqCst);
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                peak_running.fetch_max(now, Ordering::SeqCst);
                // Page 0 is slow: later pages must not run arbitrarily far ahead of it.
                thread::sleep(Duration::from_millis(if index == 0 { 60 } else { 1 }));
                running.fetch_sub(1, Ordering::SeqCst);
                Ok(index)
            },
            |index, _| {
                peak_ahead = peak_ahead.max(started.load(Ordering::SeqCst) - index);
                Ok(())
            },
        )
        .unwrap();
        assert!(peak_running.load(Ordering::SeqCst) <= WORKERS);
        // Started-but-unconsumed pages (running plus buffered) never exceed the worker count.
        assert!(peak_ahead <= WORKERS, "peak {peak_ahead}");
    }

    #[test]
    fn lanes_are_distinct_and_bounded() {
        let lanes = Mutex::new(std::collections::BTreeSet::new());
        run_ordered::<_, ()>(
            20,
            4,
            &CancellationToken::new(),
            |lane, _| {
                lanes.lock().unwrap().insert(lane);
                thread::sleep(Duration::from_millis(2));
                Ok(())
            },
            |_, _| Ok(()),
        )
        .unwrap();
        assert!(lanes.lock().unwrap().iter().all(|lane| *lane < 4));
    }

    #[test]
    fn first_failure_in_page_order_wins_and_stops_new_pages() {
        let stop = CancellationToken::new();
        let started = AtomicUsize::new(0);
        let error = run_ordered(
            200,
            4,
            &stop,
            |_, index| {
                started.fetch_add(1, Ordering::SeqCst);
                if index == 5 {
                    thread::sleep(Duration::from_millis(20));
                    return Err("page 5");
                }
                if index == 7 {
                    return Err("page 7");
                }
                thread::sleep(Duration::from_millis(1));
                Ok(index)
            },
            |_, _| Ok(()),
        )
        .unwrap_err();
        assert_eq!(
            error, "page 5",
            "sequential order decides which error is reported"
        );
        assert!(
            stop.is_cancelled(),
            "in-flight engine calls must be cancelled"
        );
        assert!(
            started.load(Ordering::SeqCst) < 200,
            "no further pages after a failure"
        );
    }

    #[test]
    fn consumer_failure_stops_the_run() {
        let stop = CancellationToken::new();
        let mut consumed = 0;
        let error = run_ordered(
            50,
            4,
            &stop,
            |_, index| Ok::<_, &str>(index),
            |index, _| {
                consumed += 1;
                if index == 3 { Err("consume") } else { Ok(()) }
            },
        )
        .unwrap_err();
        assert_eq!(error, "consume");
        assert_eq!(consumed, 4);
        assert!(stop.is_cancelled());
    }

    #[test]
    fn empty_job_is_a_no_op() {
        run_ordered::<(), ()>(
            0,
            4,
            &CancellationToken::new(),
            |_, _| Ok(()),
            |_, _| Ok(()),
        )
        .unwrap();
    }
}
