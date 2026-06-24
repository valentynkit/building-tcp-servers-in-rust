use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::Config;

/// A flag every backend's loop checks to know when to stop. Cloning shares the
/// flag, so the test thread sets it and the server thread sees it.
#[derive(Clone, Default)]
pub struct ShutdownSignal(Arc<AtomicBool>);

impl ShutdownSignal {
    pub fn new() -> Self {
        ShutdownSignal(Arc::new(AtomicBool::new(false)))
    }

    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// One TCP server. `bind` opens the listener, `local_addr` reports where it
/// landed (so a test on port 0 learns the real port), `serve` runs the loop
/// until the shutdown signal is set.
pub trait Server: Sized + Send + 'static {
    fn bind(cfg: &Config) -> io::Result<Self>;
    fn local_addr(&self) -> SocketAddr;
    fn serve(self, shutdown: ShutdownSignal) -> io::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_starts_unset_and_latches() {
        let s = ShutdownSignal::new();
        assert!(!s.is_stopped());
        let clone = s.clone();
        s.stop();
        assert!(clone.is_stopped(), "clones share the same flag");
    }
}
