//! Orders session revocation and browser administration in a single Service process.
//!
//! A request's lease is cloned into blocking database workers. Cancellation of
//! the HTTP future cannot release the ordering boundary before a worker commits.

use std::{future::Future, sync::Arc, time::Duration};

use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};
use tokio::time::timeout;

const GATE_QUEUE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Default)]
pub(crate) struct SessionGate {
    lock: Arc<RwLock<()>>,
}

pub(crate) enum SessionLease {
    Administration { _guard: OwnedRwLockReadGuard<()> },
    Authentication { _guard: OwnedRwLockWriteGuard<()> },
}

tokio::task_local! {
    static REQUEST_LEASE: Arc<SessionLease>;
}

impl SessionGate {
    pub(crate) async fn acquire(&self, write: bool) -> Result<Arc<SessionLease>, &'static str> {
        if write {
            let guard = timeout(GATE_QUEUE_TIMEOUT, Arc::clone(&self.lock).write_owned())
                .await
                .map_err(|_| "session change queue timed out; retry later")?;
            Ok(Arc::new(SessionLease::Authentication { _guard: guard }))
        } else {
            let guard = timeout(GATE_QUEUE_TIMEOUT, Arc::clone(&self.lock).read_owned())
                .await
                .map_err(|_| "administration queue timed out; retry later")?;
            Ok(Arc::new(SessionLease::Administration { _guard: guard }))
        }
    }

    #[cfg(test)]
    pub(crate) fn write_available(&self) -> bool {
        self.lock.try_write().is_ok()
    }
}

pub(crate) async fn scope<T>(
    lease: Option<Arc<SessionLease>>,
    future: impl Future<Output = T>,
) -> T {
    match lease {
        Some(lease) => REQUEST_LEASE.scope(lease, future).await,
        None => future.await,
    }
}

/// No lease is expected for local privileged CLI calls or background maintenance.
/// Every HTTP administration path enters its scope in the root request guard.
pub(crate) fn current() -> Option<Arc<SessionLease>> {
    REQUEST_LEASE.try_with(Arc::clone).ok()
}
