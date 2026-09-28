//! Workload and convergence checks shared by the borders benchmark and CPU regressions.
use emerge_skia::renderer::RendererCachePaintLayerFrameStats as CacheStats;
use emerge_skia::tree::element::{ElementTree, NodeId};
use emerge_skia::tree::geometry::Rect;

// Identities belong to rich_box_shadow_section(base=55_000), not render-layer topology.
pub const ANIMATED_CARDS: [u64; 3] = [65_000, 66_000, 67_000];
pub const STATIC_RECIPE: u64 = 75_000;

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub scroll_y: f32,
    pub animated_regions: [Rect; 3],
    pub static_region: Rect,
}

pub fn select_viewport(tree: &ElementTree, scroll_id: NodeId, width: u32, height: u32) -> Viewport {
    let frame = |id| {
        Rect::from_frame(
            tree.get(&NodeId::from_u64(id))
                .expect("fixture element missing")
                .layout
                .frame
                .expect("fixture must be laid out"),
        )
    };
    let cards = ANIMATED_CARDS.map(frame);
    let top = cards.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
    let bottom = cards
        .iter()
        .map(|r| r.y + r.height)
        .fold(f32::NEG_INFINITY, f32::max);
    let scroll = tree.get(&scroll_id).expect("scroll shell missing");
    let origin = scroll.layout.frame.expect("scroll shell must be laid out");
    let scroll_y = ((top + bottom) * 0.5 - origin.y - height as f32 * 0.5)
        .clamp(0.0, scroll.layout.scroll_y_max);
    let viewport = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let local = |r: Rect| r.offset(origin.x, origin.y + scroll_y);
    // Include shadow corners: short horizontal orbit segments can leave the
    // middle of a wide card unchanged even though its shadow is moving.
    let animated_regions = cards.map(|r| {
        local(Rect {
            x: r.x,
            y: r.y - 32.0,
            width: r.width,
            height: r.height + 64.0,
        })
    });
    let recipe = local(frame(STATIC_RECIPE));
    // Sample inside the opaque recipe, away from neighboring shadow fringes.
    let static_region = Rect {
        x: recipe.x + 16.0,
        y: recipe.y + 16.0,
        width: recipe.width - 32.0,
        height: recipe.height - 32.0,
    };
    for region in cards
        .map(local)
        .into_iter()
        .chain(animated_regions)
        .chain([recipe, static_region])
    {
        assert!(
            region.width > 0.0
                && region.height > 0.0
                && viewport.contains(region.x, region.y)
                && viewport.contains(region.x + region.width, region.y + region.height),
            "fixture coverage outside viewport: {region:?}"
        );
    }
    Viewport {
        scroll_y,
        animated_regions,
        static_region,
    }
}

pub fn is_steady(stats: CacheStats) -> bool {
    stats.hits > 0
        && stats.misses <= 2
        && stats.stores <= 2
        && stats.rejected_payload_budget == 0
        && stats.evictions == 0
        && stats.stale_evictions == 0
        && stats.prepare_failures == 0
}

/// Return the next animation-state index, after a whole consecutive steady cycle.
/// The bound allows every state's initially visible working set to fill at the
/// configured budget, plus two cycles for admission and convergence. Never wait
/// indefinitely for a continuously changing or evicting cache.
pub fn warm_cache(
    first: CacheStats,
    budget: u32,
    state_count: usize,
    mut render: impl FnMut(usize) -> CacheStats,
) -> usize {
    assert!(
        budget > 0 && state_count > 1,
        "warm-up needs a budget and animation states"
    );
    assert!(first.stores > 0, "no cold-cache payloads stored: {first:?}");
    let working_set = usize::try_from(first.visible_candidates)
        .expect("candidate count overflow")
        .checked_mul(state_count)
        .expect("working set overflow");
    let limit = working_set
        .div_ceil(budget as usize)
        .checked_add(2 * state_count)
        .expect("warm-up bound overflow");
    let mut last = first;
    let mut consecutive = usize::from(is_steady(first));
    for frame in 1..limit {
        last = render(frame % state_count);
        consecutive = if is_steady(last) { consecutive + 1 } else { 0 };
        if consecutive == state_count {
            return (frame + 1) % state_count;
        }
    }
    panic!(
        "borders cache did not converge within {limit} frames (budget={budget}, states={state_count}, consecutive={consecutive}, cold={first:?}, last={last:?})"
    );
}

pub struct CoverageSamples {
    pub animated: [Vec<u8>; 3],
    pub static_paint: Vec<u8>,
}

pub fn assert_coverage(first: &CoverageSamples, last: &CoverageSamples) {
    for (id, (a, b)) in ANIMATED_CARDS
        .into_iter()
        .zip(first.animated.iter().zip(&last.animated))
    {
        assert!(
            !a.is_empty() && a.len() == b.len(),
            "missing card pixels: {id}"
        );
        assert!(a != b, "animated card {id} has no visible pixel change");
    }
    assert!(
        first.static_paint == last.static_paint,
        "static recipe changed across animation states"
    );
    let pixels = &first.static_paint;
    assert!(
        pixels.len() >= 8 && pixels.as_chunks::<4>().0.iter().any(|p| p != &pixels[..4]),
        "static recipe must contain painted detail, not a blank backdrop"
    );
}

/// Synchronous setup-only readbacks; never call this inside a timed iteration.
pub fn read_coverage(surface: &mut skia_safe::Surface, viewport: Viewport) -> CoverageSamples {
    let mut read = |rect: Rect| {
        let x = rect.x.floor() as i32;
        let y = rect.y.floor() as i32;
        let width = (rect.x + rect.width).ceil() as i32 - x;
        let height = (rect.y + rect.height).ceil() as i32 - y;
        let info = skia_safe::ImageInfo::new_n32_premul((width, height), None);
        let mut pixels = vec![0; width as usize * height as usize * 4];
        assert!(
            surface.read_pixels(&info, pixels.as_mut_slice(), width as usize * 4, (x, y)),
            "borders coverage readback failed"
        );
        pixels
    };
    CoverageSamples {
        animated: viewport.animated_regions.map(&mut read),
        static_paint: read(viewport.static_region),
    }
}
