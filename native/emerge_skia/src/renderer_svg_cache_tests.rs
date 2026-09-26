use super::*;
use crate::assets::{self, AssetConfig, AssetRuntime, AssetStatus};
use crate::tree::attrs::{Attrs, ImageSource};
use crate::tree::element::{Element, ElementKind, ElementTree, NodeId};
use skia_safe::image::CachingHint;

fn config() -> AssetConfig {
    AssetConfig {
        sources: vec![format!("{}/../../priv", env!("CARGO_MANIFEST_DIR"))],
        ..AssetConfig::default()
    }
}

fn source_tree(source: &ImageSource) -> ElementTree {
    let id = NodeId::from_u64(1);
    let mut tree = ElementTree::new();
    tree.insert(Element::with_attrs(
        id,
        ElementKind::Image,
        vec![],
        Attrs {
            image_src: Some(source.clone()),
            ..Attrs::default()
        },
    ));
    tree.set_root_id(id);
    tree
}

fn load(source: &ImageSource) -> (ElementTree, String) {
    let tree = source_tree(source);
    assets::resolve_tree_sources_sync(&tree, None).unwrap();
    let Some(AssetStatus::Ready(asset)) = assets::source_status(source) else {
        panic!("not ready");
    };
    (tree, asset.id)
}

fn draw(id: &str, size: u32, fit: ImageFit, profiled: bool) -> Vec<u8> {
    let mut surface = skia_safe::surfaces::raster_n32_premul((size as i32, size as i32)).unwrap();
    surface.canvas().clear(Color::TRANSPARENT);
    let spec = ImageDrawSpec {
        rect: RectSpec {
            x: 0.0,
            y: 0.0,
            w: size as f32,
            h: size as f32,
        },
        image_id: id,
        fit,
        svg_tint: None,
    };
    if profiled {
        draw_cached_asset_with_fit_profiled(surface.canvas(), spec, 0.0);
    } else {
        draw_cached_asset_with_fit(surface.canvas(), spec, 0.0);
    }
    let info = ImageInfo::new(
        (size as i32, size as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut pixels = vec![0; size as usize * size as usize * 4];
    assert!(surface.read_pixels(&info, &mut pixels, size as usize * 4, (0, 0)));
    pixels
}

#[test]
fn parsed_svg_survives_scene_removal_before_first_rasterization() {
    let runtime = AssetRuntime::new();
    let _context = runtime.enter();
    runtime.configure(config());
    let source = ImageSource::Logical("test_assets/cache_complex.svg".into());
    let (tree, id) = load(&source);
    let parsed = assets::asset_record(&id).unwrap();
    assert_eq!(assets::svg_cache_stats().parses, 1);
    assert_eq!(assets::svg_cache_stats().rasterizations, 0);
    assets::ensure_tree_sources(&ElementTree::new());
    assets::ensure_tree_sources(&tree);
    assert!(matches!(
        assets::source_status(&source),
        Some(AssetStatus::Ready(_))
    ));
    assert!(Arc::ptr_eq(&parsed, &assets::asset_record(&id).unwrap()));

    for fit in [ImageFit::Contain, ImageFit::Cover] {
        let images: Vec<_> = [24, 68, 200]
            .into_iter()
            .map(|size| {
                assets::ensure_tree_sources(&ElementTree::new());
                assets::ensure_tree_sources(&tree);
                draw(&id, size, fit, false)
            })
            .collect();
        for (index, size) in [24, 68, 200].into_iter().enumerate() {
            assets::ensure_tree_sources(&ElementTree::new());
            assets::ensure_tree_sources(&tree);
            assert_eq!(images[index], draw(&id, size, fit, true));
            // Force a fresh vector rasterization at this exact size, not scaled pixels.
            let record = assets::asset_record(&id).unwrap();
            let crate::assets::AssetRecordKind::Vector(vector) = &record.kind else {
                unreachable!();
            };
            let fresh = if fit == ImageFit::Cover {
                rasterize_vector_tree_cover_viewport(vector, size, size)
            } else {
                rasterize_vector_tree(vector, size, size)
            }
            .unwrap();
            let info = ImageInfo::new(
                (size as i32, size as i32),
                ColorType::RGBA8888,
                AlphaType::Premul,
                None,
            );
            let mut expected = vec![0; size as usize * size as usize * 4];
            assert!(fresh.read_pixels(
                &info,
                &mut expected,
                size as usize * 4,
                (0, 0),
                CachingHint::Allow
            ));
            assert_eq!(images[index], expected);
        }
    }
    let stats = assets::svg_cache_stats();
    assert_eq!(stats.font_discoveries, 1);
    assert_eq!(stats.parses, 1);
    assert_eq!(stats.rasterizations, 12); // six cache fills + six fresh references
    assert_eq!(asset_memory_stats_snapshot().vector_cache_entries, 6);
}

#[test]
fn svg_tree_and_pixels_are_independently_evictable() {
    let runtime = AssetRuntime::new();
    let _context = runtime.enter();
    runtime.configure(config());
    let source = ImageSource::Logical("test_assets/cache_complex.svg".into());
    let (tree, id) = load(&source);
    let first = draw(&id, 68, ImageFit::Contain, false);
    clear_cached_svg_pixels();
    assets::ensure_tree_sources(&ElementTree::new());
    assets::ensure_tree_sources(&tree);
    assert_eq!(draw(&id, 68, ImageFit::Contain, true), first);
    assert_eq!(assets::svg_cache_stats().parses, 1);
    assert_eq!(assets::svg_cache_stats().rasterizations, 2);

    assets::ensure_tree_sources(&ElementTree::new());
    assets::remove_asset_record(&id); // explicitly evict parsed tree, not pixel entry
    assets::ensure_tree_sources(&tree);
    assert!(assets::asset_record(&id).is_none());
    assert_eq!(asset_kind(&id), Some(AssetKind::Vector));
    assert_eq!(draw(&id, 68, ImageFit::Contain, false), first);
    assert_eq!(draw(&id, 68, ImageFit::Contain, true), first);
    assert_eq!(assets::svg_cache_stats().rasterizations, 2);
    assert_eq!(assets::svg_cache_stats().parses, 1);

    // A new size hydrates asynchronously, retaining the old exact-size pixels.
    let (tx, rx) = crossbeam_channel::bounded(16);
    runtime.start(tx, false);
    assets::ensure_tree_sources(&tree);
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(); // revalidation, no parse
    assert_eq!(assets::svg_cache_stats().parses, 1);
    draw(&id, 200, ImageFit::Contain, true);
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(); // requested tree hydration
    draw(&id, 200, ImageFit::Contain, false);
    assert_eq!(assets::svg_cache_stats().parses, 2);
    assert_eq!(assets::svg_cache_stats().font_discoveries, 1);
    assert_eq!(asset_memory_stats_snapshot().vector_cache_entries, 2);
    assert_eq!(draw(&id, 68, ImageFit::Contain, true), first);
    runtime.stop();
}

#[test]
fn raster_and_svg_pixels_share_one_lru_and_byte_budget() {
    let runtime = AssetRuntime::new();
    let _context = runtime.enter();
    runtime.configure(AssetConfig {
        cache_max_entries: 2,
        cache_max_bytes: 2 * 16 * 16 * 4,
        ..config()
    });
    let source = ImageSource::Logical("test_assets/cache_complex.svg".into());
    let (_, id) = load(&source);
    draw(&id, 16, ImageFit::Contain, false);
    insert_test_raster_asset_rgba("raster", 16, 16, &vec![255; 16 * 16 * 4]).unwrap();
    let record = assets::asset_record("raster").unwrap();
    preload_raster_asset_original(&record.id).unwrap();
    assert!(retained_asset_metadata("raster").is_some());
    draw(&id, 16, ImageFit::Contain, false); // SVG becomes newer than raster
    draw(&id, 8, ImageFit::Contain, false); // evicts raster, not newer SVG
    assert!(retained_asset_metadata("raster").is_none());
    let snapshot = asset_memory_stats_snapshot();
    assert_eq!(snapshot.vector_cache_entries, 2);
    assert_eq!(snapshot.raster_cache_entries, 0);
    assert_eq!(snapshot.vector_cache_bytes, (16 * 16 + 8 * 8) * 4);
    configure_asset_cache(10, 8 * 8 * 4);
    assert_eq!(asset_memory_stats_snapshot().vector_cache_entries, 1);
    configure_asset_cache(0, 0);
    draw(&id, 68, ImageFit::Contain, false);
    assert_eq!(asset_memory_stats_snapshot().vector_cache_entries, 0);
}

#[test]
fn same_content_aliases_share_parsing_and_documents_share_font_discovery() {
    let runtime = AssetRuntime::new();
    let _context = runtime.enter();
    runtime.configure(config());
    let first = ImageSource::Logical("test_assets/cache_complex.svg".into());
    let alias = ImageSource::Logical("/test_assets/cache_complex.svg".into());
    let (_, first_id) = load(&first);
    let (_, alias_id) = load(&alias);
    assert_eq!(first_id, alias_id);
    assert_eq!(assets::svg_cache_stats().parses, 1);
    let font_bytes = assets::svg_cache_stats().font_estimated_bytes;
    assert!(font_bytes > 0);
    let (_, second_id) = load(&ImageSource::Logical(
        "test_assets/single_axis_square.svg".into(),
    ));
    let first_record = assets::asset_record(&first_id).unwrap();
    let second_record = assets::asset_record(&second_id).unwrap();
    let (assets::AssetRecordKind::Vector(first_tree), assets::AssetRecordKind::Vector(second_tree)) =
        (&first_record.kind, &second_record.kind)
    else {
        panic!("not vectors");
    };
    assert!(Arc::ptr_eq(first_tree.fontdb(), second_tree.fontdb()));
    assert_eq!(assets::svg_cache_stats().font_estimated_bytes, font_bytes);
    assert_eq!(assets::svg_cache_stats().parses, 2);
    assert_eq!(assets::svg_cache_stats().font_discoveries, 1);
}

#[test]
fn disabled_and_small_parsed_budgets_do_not_disable_font_reuse() {
    for (entries, bytes) in [(0, u64::MAX), (64, 1)] {
        let runtime = AssetRuntime::new();
        let _context = runtime.enter();
        runtime.configure(AssetConfig {
            svg_tree_max_entries: entries,
            svg_tree_max_bytes: bytes,
            ..config()
        });
        let source = ImageSource::Logical("test_assets/cache_complex.svg".into());
        load(&source);
        assert_eq!(assets::svg_cache_stats().entries, 0);
        assets::ensure_tree_sources(&ElementTree::new());
        load(&source);
        assert_eq!(assets::svg_cache_stats().parses, 2);
        assert_eq!(assets::svg_cache_stats().font_discoveries, 1);
    }
}

#[test]
fn svg_render_revisions_survive_scene_changes_and_invalidate_missing_variants() {
    let runtime = AssetRuntime::new();
    let _context = runtime.enter();
    runtime.configure(config());
    let source = ImageSource::Logical("test_assets/cache_complex.svg".into());
    let (tree, id) = load(&source);
    draw(&id, 24, ImageFit::Contain, false);
    let fingerprint_before_eviction = asset_render_generation(&id);
    let revision = assets::asset_record(&id).unwrap().render_revision.clone();
    assets::ensure_tree_sources(&ElementTree::new());
    assets::remove_asset_record(&id);
    invalidate_asset_render_revision(&id); // miss discovered after a layer fingerprint
    load(&source);
    assert!(Arc::ptr_eq(
        &revision,
        &assets::asset_record(&id).unwrap().render_revision
    ));
    assert_ne!(asset_render_generation(&id), fingerprint_before_eviction);
    let before_new_variant = asset_render_generation(&id);
    draw(&id, 68, ImageFit::Contain, false);
    let finished = asset_render_generation(&id);
    assert_ne!(finished, before_new_variant);
    assets::ensure_tree_sources(&ElementTree::new());
    assets::ensure_tree_sources(&tree);
    assert_eq!(asset_render_generation(&id), finished);
    draw(&id, 68, ImageFit::Contain, true);
    assert_eq!(asset_render_generation(&id), finished); // hits do not invalidate layers
    assets::remove_asset_record(&id);
    let pixel_only = asset_render_generation(&id);
    draw(&id, 68, ImageFit::Contain, true);
    assert_eq!(asset_render_generation(&id), pixel_only);
}
