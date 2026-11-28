use super::*;
use crate::session::presentation::Id;

// One wl_surface.frame request. A reconfigure, rejected claim or an early
// callback must never supply another frame's latency sample. A device report
// can arrive after the callback, so join by serial, not report arrival time.
pub(in crate::session) struct Callback {
    id: Id,
    accepted: Option<Instant>,
    handoff: Option<Instant>,
    feedback: Option<Instant>,
}
impl Callback {
    pub(in crate::session) fn arm(b: &Bundle) -> Option<Self> {
        b.queued.map(|_| Self {
            id: b.id,
            accepted: b.accepted,
            handoff: None,
            feedback: None,
        })
    }
    pub(in crate::session) fn handed(
        slot: &mut Option<Self>,
        id: Id,
        at: Instant,
        stats: &Collector,
    ) {
        if let Some(c) = slot
            && c.id == id
        {
            c.handoff = Some(at);
            Self::finish(slot, stats);
        }
    }
    pub(in crate::session) fn received(slot: &mut Option<Self>, at: Instant, stats: &Collector) {
        if let Some(c) = slot {
            c.feedback = Some(at);
            Self::finish(slot, stats);
        }
    }
    fn finish(slot: &mut Option<Self>, stats: &Collector) {
        let Some(c) = slot else { return };
        let Some((handed, done)) = c.handoff.zip(c.feedback) else {
            return;
        };
        if let Some(wait) = done.checked_duration_since(handed) {
            stats.update(|w| {
                w.counts.timings.record(Metric::CallbackWait, wait);
                if let Some(start) = c.accepted {
                    w.counts
                        .timings
                        .record(Metric::CallbackTotal, done.duration_since(start));
                }
            });
        }
        *slot = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_paired_successful_handoffs_contribute_even_if_the_report_arrives_last() {
        let s = Collector::new(true, false, None);
        let id = Id {
            generation: 1,
            serial: 2,
        };
        let start = Instant::now();
        let pending = || {
            Some(Callback {
                id,
                accepted: Some(start),
                handoff: None,
                feedback: None,
            })
        };
        let mut c = pending();
        let at = |ms| start + Duration::from_millis(ms);
        Callback::received(&mut c, at(10), &s);
        Callback::handed(
            &mut c,
            Id {
                generation: 2,
                ..id
            },
            at(7),
            &s,
        );
        assert_eq!(
            s.reading(StatsCommandNif::Peek)
                .counts
                .timings
                .pipeline
                .count,
            0
        );
        Callback::handed(&mut c, id, at(7), &s);
        assert!(c.is_none());
        let t = s.reading(StatsCommandNif::Take).counts.timings;
        assert_eq!(
            (
                t.pipeline.count,
                t.pipeline.avg_ms,
                t.pipeline_swap_to_frame_callback.avg_ms
            ),
            (1, 10., 3.)
        );
        let mut c = pending();
        Callback::handed(&mut c, id, at(15), &s);
        Callback::received(&mut c, at(14), &s);
        assert!(c.is_none());
        assert_eq!(
            s.reading(StatsCommandNif::Peek)
                .counts
                .timings
                .pipeline_swap_to_frame_callback
                .count,
            0
        );
    }
}
