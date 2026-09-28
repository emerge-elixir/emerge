use super::*;
use crate::assets::{AssetConfig, AssetRuntime};
use crate::render_scene::RenderScene;

fn register(vector: bool, id: &str, green: bool) {
    if vector {
        let color = if green { "lime" } else { "red" };
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="{color}"/></svg>"#
        );
        insert_vector_asset(
            id,
            usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap(),
        )
        .unwrap();
    } else {
        let pixel = if green {
            [0, 255, 0, 255]
        } else {
            [255, 0, 0, 255]
        };
        insert_test_raster_asset_rgba(id, 8, 8, &pixel.repeat(64)).unwrap();
    }
}
fn node(id: &str, fit: ImageFit) -> RenderNode {
    RenderNode::Primitive(DrawPrimitive::Image(
        0.0,
        0.0,
        32.0,
        32.0,
        id.into(),
        fit,
        None,
    ))
}
fn scene(id: &str, fit: ImageFit) -> RenderScene {
    let nodes = vec![node(id, fit)];
    RenderScene {
        fonts: None,
        images: Some(ImageSnapshot::capture(&nodes)),
        nodes,
    }
}
fn draw(renderer: &mut SceneRenderer, scene: RenderScene, version: u64, profile: bool) -> Vec<u8> {
    let info = ImageInfo::new((32, 32), ColorType::RGBA8888, AlphaType::Premul, None);
    let mut surface = skia_safe::surfaces::raster(&info, None, None).unwrap();
    let state = RenderState::new(scene, skia_safe::Color::TRANSPARENT, version, false);
    let mut frame = RenderFrame::new(&mut surface, None);
    if profile {
        renderer.render_profiled(&mut frame, &state);
    } else {
        renderer.render(&mut frame, &state);
    }
    let mut pixels = vec![0; 32 * 32 * 4];
    assert!(surface.read_pixels(&info, &mut pixels[..], 32 * 4, (0, 0)));
    pixels
}

#[test]
fn replacements_before_first_paint_and_after_warming_never_rebind_old_scenes() {
    for vector in [false, true] {
        for fit in [
            ImageFit::Contain,
            ImageFit::Cover,
            ImageFit::Repeat,
            ImageFit::RepeatX,
            ImageFit::RepeatY,
        ] {
            for profile in [false, true] {
                let assets = AssetRuntime::new();
                let _assets = assets.enter();
                register(vector, "same", false);
                let old = scene("same", fit);
                let mut renderer = SceneRenderer::new();
                register(vector, "same", true);
                let new = scene("same", fit);
                let green = draw(&mut renderer, new.clone(), 1, profile);
                let red = draw(&mut renderer, old.clone(), 2, profile);
                assert_eq!(&red[..4], &[255, 0, 0, 255]);
                assert_eq!(&green[..4], &[0, 255, 0, 255]);
                assert_eq!(green, draw(&mut renderer, new, 3, profile));
                assert_eq!(red, draw(&mut renderer, old, 4, profile));
            }
        }
    }
}

#[test]
fn cached_only_bindings_survive_source_eviction_replacement_and_pixel_cache_reset() {
    for vector in [false, true] {
        let assets = AssetRuntime::new();
        let _assets = assets.enter();
        assets.configure(AssetConfig {
            svg_tree_max_entries: 0,
            svg_tree_max_bytes: 0,
            ..Default::default()
        });
        register(vector, "evicted", false);
        let mut renderer = SceneRenderer::new();
        let expected = draw(&mut renderer, scene("evicted", ImageFit::Contain), 1, false);
        crate::assets::remove_asset_record("evicted");
        assert!(crate::assets::asset_record("evicted").is_none());
        let old = scene("evicted", ImageFit::Contain);
        let stats = old.images.as_ref().unwrap().retention();
        assert_eq!(stats.source_records, 0);
        assert!(stats.cached_pixel_bytes > 0);
        register(vector, "evicted", true);
        clear_renderer_asset_context();
        assert_eq!(expected, draw(&mut renderer, old, 2, true));
    }
}

#[test]
fn renderer_local_generation_collisions_do_not_alias_pixels_or_pollute_live_metadata() {
    for vector in [false, true] {
        let a = AssetRuntime::new();
        let b = AssetRuntime::new();
        let (old, generation) = {
            let _a = a.enter();
            register(vector, "collision", false);
            (
                scene("collision", ImageFit::Contain),
                crate::assets::asset_record("collision").unwrap().generation,
            )
        };
        let _b = b.enter();
        register(vector, "collision", true);
        let record = crate::assets::asset_record("collision").unwrap();
        assert_eq!(record.generation, generation);
        let new = scene("collision", ImageFit::Contain);
        let mut renderer = SceneRenderer::new();
        let green = draw(&mut renderer, new.clone(), 1, false);
        let red = draw(&mut renderer, old, 2, true);
        assert_eq!(&red[..4], &[255, 0, 0, 255]);
        assert_eq!(&green[..4], &[0, 255, 0, 255]);
        let metadata = retained_asset_metadata("collision").unwrap();
        assert!(Arc::ptr_eq(
            &metadata.render_revision.unwrap(),
            &record.render_revision
        ));
        assert_eq!(green, draw(&mut renderer, new, 3, false));
    }
}

#[test]
fn source_pins_are_sparse_shared_and_released_with_the_last_scene() {
    for vector in [false, true] {
        let assets = AssetRuntime::new();
        let _assets = assets.enter();
        register(vector, "used", false);
        register(vector, "unused", true);
        let used = Arc::downgrade(&crate::assets::asset_record("used").unwrap());
        let unused = Arc::downgrade(&crate::assets::asset_record("unused").unwrap());
        let nodes = vec![RenderNode::ShadowPass {
            children: vec![
                node("used", ImageFit::Contain),
                node("used", ImageFit::Cover),
            ],
        }];
        let snapshot = ImageSnapshot::capture(&nodes);
        let clone = snapshot.clone();
        assert!(Arc::ptr_eq(&snapshot.0, &clone.0));
        let stats = snapshot.retention();
        assert_eq!(stats.bindings, 1);
        assert_eq!(stats.source_records, 1);
        assert_eq!(stats.cached_pixel_bytes, 0);
        if !vector {
            assert!(stats.encoded_bytes > 0);
        }
        assets.stop();
        assert!(used.upgrade().is_some());
        assert!(unused.upgrade().is_none());
        let mut renderer = SceneRenderer::new();
        let state = RenderScene {
            fonts: None,
            images: Some(snapshot),
            nodes,
        };
        assert_eq!(
            &draw(&mut renderer, state, 1, false)[..4],
            &[255, 0, 0, 255]
        );
        assert!(used.upgrade().is_some());
        drop(clone);
        assert!(used.upgrade().is_none());
    }
}

#[test]
fn frozen_absence_and_nested_manual_scenes_never_borrow_an_outer_binding() {
    let assets = AssetRuntime::new();
    let _assets = assets.enter();
    let missing = scene("later", ImageFit::Contain);
    let mut renderer = SceneRenderer::new();
    let before = draw(&mut renderer, missing.clone(), 1, false);
    register(true, "later", true);
    assert_eq!(before, draw(&mut renderer, missing.clone(), 2, true));
    let _outer = enter(missing.images.as_ref());
    assert!(record("later").is_none());
    {
        let _manual = enter(None);
        assert!(record("later").is_some());
    }
    assert!(record("later").is_none());
    let fresh = scene("later", ImageFit::Contain);
    assert_eq!(
        &draw(&mut renderer, fresh, 3, false)[..4],
        &[0, 255, 0, 255]
    );
    assert!(record("later").is_none());
}

#[test]
fn newest_cached_version_is_selected_and_empty_asset_universe_skips_graph_walk() {
    let assets = AssetRuntime::new();
    let _assets = assets.enter();
    let nodes = vec![node("absent", ImageFit::Contain); 1000];
    let empty = ImageSnapshot::capture(&nodes);
    assert_eq!(empty.capture_node_visits(), 0);
    register(true, "versions", false);
    let mut renderer = SceneRenderer::new();
    draw(
        &mut renderer,
        scene("versions", ImageFit::Contain),
        1,
        false,
    );
    // Use the registry directly so both generations' raster variants remain.
    let green=usvg::Tree::from_str("<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'><rect width='8' height='8' fill='lime'/></svg>",&usvg::Options::default()).unwrap();
    let record = crate::assets::register_vector_asset("versions", green).unwrap();
    let expected = draw(
        &mut renderer,
        scene("versions", ImageFit::Contain),
        2,
        false,
    );
    crate::assets::remove_asset_record("versions");
    assert_eq!(
        retained_asset_metadata("versions").unwrap().generation,
        record.generation
    );
    let cached = scene("versions", ImageFit::Contain);
    assert_eq!(cached.images.as_ref().unwrap().capture_node_visits(), 1);
    register(true, "versions", false);
    assert_eq!(expected, draw(&mut renderer, cached, 3, true));
}

#[test]
fn grayscale_policy_uses_the_captured_kind_and_capture_does_no_render_work() {
    let assets = AssetRuntime::new();
    let _assets = assets.enter();
    register(true, "kind", false);
    let before = crate::assets::svg_cache_stats();
    let source_generation = crate::assets::source_status_generation();
    let old = scene("kind", ImageFit::Contain);
    assert_eq!(
        crate::assets::svg_cache_stats().rasterizations,
        before.rasterizations
    );
    assert_eq!(crate::assets::svg_cache_stats().parses, before.parses);
    assert_eq!(crate::assets::source_status_generation(), source_generation);
    let renderer = SceneRenderer::new();
    let state = |scene| RenderState::new(scene, skia_safe::Color::TRANSPARENT, 1, false);
    let old_policy = renderer
        .render_grayscale_dither_policy(32, 32, &state(old.clone()))
        .unwrap();
    register(false, "kind", true);
    let new_policy = renderer
        .render_grayscale_dither_policy(32, 32, &state(scene("kind", ImageFit::Contain)))
        .unwrap();
    assert_ne!(old_policy, new_policy);
    assert_eq!(
        old_policy,
        renderer
            .render_grayscale_dither_policy(32, 32, &state(old.clone()))
            .unwrap()
    );
    let _guard = enter(old.images.as_ref());
    assert!(asset_context().pixel_cache.try_lock().is_ok());
    // Asset state remains available too; the snapshot guard owns no asset lock.
    assert!(crate::assets::asset_record("kind").is_some());
}
