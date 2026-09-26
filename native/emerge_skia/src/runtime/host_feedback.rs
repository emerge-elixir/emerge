//! Bounded host-side tree/event feedback, shared with the standalone macOS host.
use crate::actors::TreeMsg;

/// Process the initial batch and up to seven feedback batches. The drain callback
/// owns the host's outgoing queue; it must only be called when another step can run.
pub fn process_host_feedback<S, E>(
    state: &mut S,
    mut messages: Vec<TreeMsg>,
    mut apply: impl FnMut(&mut S, Vec<TreeMsg>) -> Result<bool, E>,
    mut drain: impl FnMut(&mut S) -> Vec<TreeMsg>,
) -> Result<(), E> {
    for round in 0..8 {
        if !apply(state, messages)? {
            return Ok(());
        }
        // Leave the next request in its owning queue for the next host tick.
        // Draining it before this check would lose its causal receipt forever.
        if round == 7 {
            return Ok(());
        }
        messages = drain(state);
        if messages.is_empty() {
            return Ok(());
        }
    }
    Ok(())
}
