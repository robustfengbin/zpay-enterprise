//! Dedicated runtime for heavy chain-sync work.
//!
//! Incident 2026-07-24 (zpaystage): the app runs under `#[actix_web::main]`,
//! whose main runtime is single-threaded. The Orchard catch-up scan
//! (trial-decryption + witness updates over hundreds of blocks per chunk) ran
//! on that runtime and blocked its one thread for tens of seconds at a time —
//! freezing every timer and, critically, the IO reactor that services the DB
//! connections opened at boot. API handlers on actix workers then watched
//! `pool.acquire()` ping those frozen connections until the 30s deadline:
//! the observed "pool timed out while waiting for an open connection" with
//! MySQL sitting idle at 6 connections.
//!
//! Fix: ship every sync execution (boot catch-up loop, run-executor tick
//! sync, API-triggered scans) onto this dedicated 2-worker runtime living on
//! its own OS threads. Blocking crunches now stall only this runtime's
//! reactor and its own connections; the main runtime and the actix workers
//! keep breathing.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::Handle;

static SYNC_RT: OnceLock<Handle> = OnceLock::new();

fn handle() -> &'static Handle {
    SYNC_RT.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("chain-sync-rt".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_name("chain-sync-worker")
                    .enable_all()
                    .build()
                    .expect("failed to build chain-sync runtime");
                tx.send(rt.handle().clone())
                    .expect("failed to hand out chain-sync runtime handle");
                // Park forever driving the runtime; tasks arrive via Handle::spawn.
                rt.block_on(std::future::pending::<()>());
            })
            .expect("failed to spawn chain-sync runtime thread");
        rx.recv().expect("chain-sync runtime thread died during startup")
    })
}

/// Run a future to completion on the dedicated sync runtime and await its
/// result from any other runtime. The future must be `Send + 'static`
/// (capture an `Arc` of your service, not `&self`).
pub async fn run<F, T>(fut: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    handle()
        .spawn(fut)
        .await
        .expect("chain-sync task panicked")
}

/// Fire-and-forget a long-lived loop onto the dedicated sync runtime.
pub fn spawn_loop<F>(fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    handle().spawn(fut);
}
