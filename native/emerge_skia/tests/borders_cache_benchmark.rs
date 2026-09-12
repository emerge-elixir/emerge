#[path = "../benches/support/mod.rs"]
mod support;

use emerge_skia::renderer::RendererCachePaintLayerFrameStats as CacheStats;
use emerge_skia::tree::element::NodeId;
use emerge_skia::tree::layout::{Constraint, layout_and_refresh_default};
use support::borders_cache::{self, CoverageSamples};

#[test]
fn viewport_tracks_animated_geometry_and_keeps_a_static_recipe_visible() {
    let assets = emerge_skia::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let mut tree = support::scrollable_rich_borders_shadow_showcase();
    layout_and_refresh_default(&mut tree, Constraint::new(960.0, 900.0), 1.0);
    let scroll_id = tree.root_id().unwrap();
    let viewport = borders_cache::select_viewport(&tree, scroll_id, 960, 900);
    for id in borders_cache::ANIMATED_CARDS {
        let element = tree.get(&NodeId::from_u64(id)).unwrap();
        assert!(element.spec.declared.animate.is_some());
        let frame = element.layout.frame.unwrap();
        assert!(frame.y >= viewport.scroll_y);
        assert!(frame.y + frame.height <= viewport.scroll_y + 900.0);
    }
    assert!(
        tree.get(&NodeId::from_u64(borders_cache::STATIC_RECIPE))
            .unwrap()
            .spec
            .declared
            .animate
            .is_none()
    );
    // Moving the authored content must move the selected viewport too. There is
    // no dependence on 1784/5008, node counts or historical paint-layer topology.
    for id in borders_cache::ANIMATED_CARDS
        .into_iter()
        .chain([borders_cache::STATIC_RECIPE])
    {
        tree.get_mut(&NodeId::from_u64(id))
            .unwrap()
            .layout
            .frame
            .as_mut()
            .unwrap()
            .y += 400.0;
    }
    let moved = borders_cache::select_viewport(&tree, scroll_id, 960, 900);
    assert_eq!(moved.scroll_y, viewport.scroll_y + 400.0);
    assert_eq!(moved.animated_regions, viewport.animated_regions);
    assert_eq!(moved.static_region, viewport.static_region);
}

fn cold(budget: u64) -> CacheStats {
    CacheStats {
        visible_candidates: 50,
        misses: 50,
        stores: budget,
        rejected_payload_budget: 50 - budget,
        ..Default::default()
    }
}

fn steady() -> CacheStats {
    CacheStats {
        hits: 50,
        ..Default::default()
    }
}

#[test]
fn default_budget_waits_for_a_complete_steady_cycle_not_two_warmup_frames() {
    let mut calls = 0;
    let next = borders_cache::warm_cache(cold(16), 16, 8, |index| {
        calls += 1;
        assert_eq!(index, calls % 8);
        match calls {
            1 => CacheStats {
                hits: 16,
                misses: 34,
                stores: 16,
                rejected_payload_budget: 18,
                ..Default::default()
            },
            2 => CacheStats {
                hits: 32,
                misses: 18,
                stores: 16,
                rejected_payload_budget: 2,
                ..Default::default()
            },
            3 => CacheStats {
                hits: 48,
                misses: 2,
                stores: 2,
                ..Default::default()
            },
            _ => steady(),
        }
    });
    assert_eq!(calls, 10);
    assert_eq!(next, 3);
}

#[test]
fn warmup_bound_scales_with_small_payload_budgets() {
    let mut calls = 0u64;
    let next = borders_cache::warm_cache(cold(1), 1, 8, |_| {
        calls += 1;
        let remaining = 50u64.saturating_sub(calls);
        CacheStats {
            hits: 50 - remaining,
            misses: remaining,
            stores: remaining.min(1),
            rejected_payload_budget: remaining.saturating_sub(1),
            ..Default::default()
        }
    });
    assert_eq!(calls, 56);
    assert_eq!(next, 1);
}

#[test]
#[should_panic(expected = "did not converge")]
fn intermittent_churn_cannot_hide_at_the_animation_cycle_boundary() {
    borders_cache::warm_cache(cold(16), 16, 8, |index| {
        if index == 7 {
            CacheStats {
                hits: 32,
                misses: 18,
                stores: 16,
                ..Default::default()
            }
        } else {
            steady()
        }
    });
}

#[test]
#[should_panic(expected = "did not converge")]
fn bypasses_without_real_hits_are_not_steady_cache_coverage() {
    borders_cache::warm_cache(cold(16), 16, 8, |_| CacheStats {
        bypassed_low_value: 50,
        ..Default::default()
    });
}

#[test]
#[should_panic(expected = "warm-up needs a budget")]
fn zero_budget_fails_instead_of_dividing_by_zero_or_waiting_forever() {
    borders_cache::warm_cache(cold(16), 0, 8, |_| steady());
}

#[test]
fn steady_checks_preserve_strict_limits_and_reject_failure_counters() {
    assert!(borders_cache::is_steady(steady()));
    for stats in [
        CacheStats {
            misses: 3,
            ..steady()
        },
        CacheStats {
            stores: 3,
            ..steady()
        },
        CacheStats {
            rejected_payload_budget: 1,
            ..steady()
        },
        CacheStats {
            evictions: 1,
            ..steady()
        },
        CacheStats {
            stale_evictions: 1,
            ..steady()
        },
        CacheStats {
            prepare_failures: 1,
            ..steady()
        },
    ] {
        assert!(!borders_cache::is_steady(stats));
    }
}

fn samples(value: u8) -> CoverageSamples {
    CoverageSamples {
        animated: std::array::from_fn(|_| vec![value; 8]),
        static_paint: vec![0, 0, 0, 255, 255, 255, 255, 255],
    }
}

#[test]
fn visible_animation_with_static_painted_detail_passes() {
    borders_cache::assert_coverage(&samples(0), &samples(1));
}

#[test]
#[should_panic(expected = "no visible pixel change")]
fn frozen_or_offscreen_animation_fails_coverage() {
    borders_cache::assert_coverage(&samples(0), &samples(0));
}

#[test]
#[should_panic(expected = "animated card 67000")]
fn one_frozen_card_is_not_hidden_by_the_other_regions() {
    let mut last = samples(1);
    last.animated[2] = vec![0; 8];
    borders_cache::assert_coverage(&samples(0), &last);
}

#[test]
#[should_panic(expected = "static recipe changed")]
fn changing_static_content_fails_coverage() {
    let mut last = samples(1);
    last.static_paint[0] = 255;
    borders_cache::assert_coverage(&samples(0), &last);
}

#[test]
#[should_panic(expected = "painted detail")]
fn blank_static_background_does_not_count_as_a_recipe() {
    let mut first = samples(0);
    let mut last = samples(1);
    first.static_paint = vec![0; 8];
    last.static_paint = vec![0; 8];
    borders_cache::assert_coverage(&first, &last);
}

#[test]
fn selected_fixture_has_real_animated_pixels_and_static_detail() {
    use emerge_skia::renderer::{RenderFrame, RenderState, SceneRenderer};
    use emerge_skia::tree::animation::AnimationRuntime;
    use emerge_skia::tree::layout::layout_and_refresh_default_with_animation;
    use std::time::{Duration, Instant};
    let assets = emerge_skia::assets::AssetRuntime::new();
    let _guard = assets.enter();
    let started = Instant::now();
    let mut tree = support::scrollable_rich_borders_shadow_showcase();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, started);
    let constraint = Constraint::new(960.0, 900.0);
    layout_and_refresh_default_with_animation(&mut tree, constraint, 1.0, &runtime, started);
    let root = tree.root_id().unwrap();
    let viewport = borders_cache::select_viewport(&tree, root, 960, 900);
    tree.apply_scroll_y(&root, -viewport.scroll_y);
    let info = skia_safe::ImageInfo::new_n32_premul((960, 900), None);
    let mut surface = skia_safe::surfaces::raster(&info, None, None).unwrap();
    let mut renderer = SceneRenderer::new();
    let samples = [0, 112].map(|ms| {
        let scene = layout_and_refresh_default_with_animation(
            &mut tree,
            constraint,
            1.0,
            &runtime,
            started + Duration::from_millis(ms),
        )
        .scene;
        let mut state = RenderState::new(scene, skia_safe::Color::WHITE, ms + 1, false);
        state.has_cacheable_paint_layers = false;
        renderer.render(&mut RenderFrame::new(&mut surface, None), &state);
        borders_cache::read_coverage(&mut surface, viewport)
    });
    borders_cache::assert_coverage(&samples[0], &samples[1]);
}
