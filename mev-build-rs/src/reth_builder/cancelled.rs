use std::sync::{atomic::AtomicBool, Arc};

// NOTE: cribbed from https://github.com/paradigmxyz/reth/blob/900ada5aaa4b5d4a633df78764e7dd7169a13405/crates/payload/basic/src/lib.rs#L514
#[derive(Default, Clone, Debug)]
pub struct Cancelled(Arc<AtomicBool>);

impl Cancelled {
    /// Returns true if the job was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for Cancelled {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
