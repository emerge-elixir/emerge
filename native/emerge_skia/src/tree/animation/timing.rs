//! Shared timing for symbolic and layout-resolved fields. No layout or owner state.
use super::{AnimationCurve, AnimationRepeat, AnimationRuntimeEntry, AnimationSpec};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SegmentPosition {
    pub index: usize,
    pub progress: f64,
    pub eased: f64,
    pub active: bool,
    /// Segment boundaries relative to the run's start, including repeat cycles.
    pub start_ms: f64,
    pub end_ms: f64,
}

/// Admission validates one loop period or the complete finite duration. Invalid
/// finite work must not masquerade as a loop just because it has no deadline.
pub(super) fn valid_duration(duration_ms: f64, repeat: &AnimationRepeat, started: Instant) -> bool {
    if !duration_ms.is_finite() || duration_ms <= 0.0 {
        return false;
    }
    let cycles = match repeat {
        AnimationRepeat::Times(n) => n.max(&1).to_owned(),
        _ => 1,
    };
    std::time::Duration::try_from_secs_f64(duration_ms * cycles as f64 / 1000.0)
        .ok()
        .filter(|duration| !duration.is_zero())
        .and_then(|duration| started.checked_add(duration))
        .is_some()
}

/// A finite, representable run deadline. Loops do not participate in a finite barrier.
pub(crate) fn finite_deadline(spec: &AnimationSpec, started: Instant) -> Option<Instant> {
    if !spec.duration_ms.is_finite() || spec.duration_ms <= 0.0 {
        return None;
    }
    let cycles = match spec.repeat {
        AnimationRepeat::Once => 1,
        AnimationRepeat::Times(n) => n.max(1),
        AnimationRepeat::Loop => return None,
    };
    let duration =
        std::time::Duration::try_from_secs_f64(spec.duration_ms * cycles as f64 / 1000.0).ok()?;
    started.checked_add(duration)
}

pub(crate) fn position(
    spec: &AnimationSpec,
    entry: Option<&AnimationRuntimeEntry>,
    sample_time: Option<Instant>,
) -> Option<SegmentPosition> {
    let elapsed = entry.zip(sample_time).map_or(0.0, |(entry, now)| {
        now.saturating_duration_since(entry.started_at)
            .as_secs_f64()
            * 1000.0
    });
    at_elapsed(spec, elapsed)
}

pub(crate) fn at_elapsed(spec: &AnimationSpec, elapsed_ms: f64) -> Option<SegmentPosition> {
    if spec.keyframes.is_empty() {
        return None;
    }
    if spec.keyframes.len() == 1 {
        return Some(SegmentPosition {
            index: 0,
            progress: 1.0,
            eased: 1.0,
            active: false,
            start_ms: 0.0,
            end_ms: 0.0,
        });
    }
    let duration = spec.duration_ms.max(f64::EPSILON);
    let elapsed = elapsed_ms.max(0.0);
    let total = match spec.repeat {
        AnimationRepeat::Once => duration,
        AnimationRepeat::Times(n) => duration * n.max(1) as f64,
        AnimationRepeat::Loop => f64::INFINITY,
    };
    let active = elapsed < total;
    let local = if active { elapsed % duration } else { duration };
    let cycle_start = if active {
        elapsed - local
    } else {
        total - duration
    };
    let segments = spec.keyframes.len() - 1;
    let p = (local / duration).clamp(0.0, 1.0) * segments as f64;
    let index = (p.floor() as usize).min(segments - 1);
    let progress = (p - index as f64).clamp(0.0, 1.0);
    Some(SegmentPosition {
        index,
        progress,
        eased: apply_curve(&spec.curve, progress),
        active,
        start_ms: cycle_start + duration * (index as f64 / segments as f64),
        end_ms: cycle_start + duration * ((index + 1) as f64 / segments as f64),
    })
}

pub(super) fn apply_curve(curve: &AnimationCurve, t: f64) -> f64 {
    match curve {
        AnimationCurve::Linear => t,
        AnimationCurve::EaseIn => t * t * t,
        AnimationCurve::EaseOut => 1.0 - (1.0 - t).powi(3),
        AnimationCurve::EaseInOut if t < 0.5 => 4.0 * t * t * t,
        AnimationCurve::EaseInOut => 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0,
    }
}

/// Continue the original curve interval after an environmental retarget. The
/// factored forms avoid subtracting nearly equal eased values near completion.
pub(crate) fn remaining_progress(curve: &AnimationCurve, anchor: f64, now: f64) -> f64 {
    let a = anchor.clamp(0.0, 1.0);
    let t = now.clamp(a, 1.0);
    if t == 1.0 {
        return 1.0;
    }
    if t == a {
        return 0.0;
    }
    let linear = (t - a) / (1.0 - a);
    match curve {
        AnimationCurve::Linear => linear,
        AnimationCurve::EaseIn => linear * (t * t + t * a + a * a) / (1.0 + a + a * a),
        AnimationCurve::EaseOut => 1.0 - ((1.0 - t) / (1.0 - a)).powi(3),
        AnimationCurve::EaseInOut if a >= 0.5 => 1.0 - ((1.0 - t) / (1.0 - a)).powi(3),
        AnimationCurve::EaseInOut if t >= 0.5 => {
            1.0 - 4.0 * (1.0 - t).powi(3) / (1.0 - 4.0 * a.powi(3))
        }
        AnimationCurve::EaseInOut => {
            4.0 * (t - a) * (t * t + t * a + a * a) / (1.0 - 4.0 * a.powi(3))
        }
    }
    .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;
    fn spec(repeat: AnimationRepeat) -> AnimationSpec {
        AnimationSpec {
            keyframes: vec![Attrs::default(); 4],
            duration_ms: 900.0,
            curve: AnimationCurve::Linear,
            repeat,
        }
    }
    #[test]
    fn boundaries_include_cycle_and_terminal_phase() {
        let once = spec(AnimationRepeat::Once);
        for (ms, index, active, start, end) in [
            (0.0, 0, true, 0.0, 300.0),
            (300.0, 1, true, 300.0, 600.0),
            (899.0, 2, true, 600.0, 900.0),
            (900.0, 2, false, 600.0, 900.0),
            (10000.0, 2, false, 600.0, 900.0),
        ] {
            let p = at_elapsed(&once, ms).unwrap();
            assert_eq!(
                (p.index, p.active, p.start_ms, p.end_ms),
                (index, active, start, end)
            );
        }
        let repeated = spec(AnimationRepeat::Times(2));
        assert_eq!(at_elapsed(&repeated, 900.0).unwrap().start_ms, 900.0);
        assert_eq!(at_elapsed(&repeated, 1800.0).unwrap().end_ms, 1800.0);
        assert!(!at_elapsed(&repeated, 1800.0).unwrap().active);
        let looped = at_elapsed(&spec(AnimationRepeat::Loop), 1800.0).unwrap();
        assert_eq!(
            (looped.index, looped.progress, looped.start_ms),
            (0, 0.0, 1800.0)
        );
    }
    #[test]
    fn remaining_curves_preserve_the_original_interval() {
        for curve in [
            AnimationCurve::Linear,
            AnimationCurve::EaseIn,
            AnimationCurve::EaseOut,
            AnimationCurve::EaseInOut,
        ] {
            for a in [0.0, 0.2, 0.5, 0.7, 0.95] {
                for fraction in [0.0, 0.1, 0.5, 0.99, 1.0] {
                    let t = a + (1.0 - a) * fraction;
                    let expected = (apply_curve(&curve, t) - apply_curve(&curve, a))
                        / (1.0 - apply_curve(&curve, a));
                    assert!((remaining_progress(&curve, a, t) - expected).abs() < 1e-10);
                }
            }
        }
        let a = 1.0 - 1e-8;
        let t = a + (1.0 - a) / 2.0;
        assert!((remaining_progress(&AnimationCurve::EaseOut, a, t) - 0.875).abs() < 1e-7);
    }
}
