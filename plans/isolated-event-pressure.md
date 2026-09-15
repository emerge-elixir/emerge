# Isolated event-thread pressure measurement

Measure current native actor paths on this host, not a proposed budget implementation.
Use production 4096-event / 512-tree channel capacities, a real event loop and tree
actor, small/20k-node static trees, paced edits/pointer/composition and a burst.
Separate healthy consumers, gated tree progress and a deliberately paused event
thread. Report offered/accepted/full-channel events, loop-boundary FIFO/outbox peaks,
requested FIFO slot storage, settling and shutdown. No RSS/GPU/BEAM bound inferred.

Test-only instrumentation and an ignored release probe; warm up before measurement.
Build and run under the exclusive performance lock, immutable source archive/hash,
three separate processes per case and raw evidence. The tree forwarding gate and
counter overhead remain in baseline cases too. No concurrent tests/builds while
measuring. Validate Rust/Mix afterwards. No commits/push or policy enablement.
