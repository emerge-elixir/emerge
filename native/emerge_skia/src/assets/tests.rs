use super::*;

fn config() -> AssetConfig {
    AssetConfig {
        sources: vec![format!("{}/../../priv", env!("CARGO_MANIFEST_DIR"))],
        ..AssetConfig::default()
    }
}

fn source(file: &str) -> ImageSource {
    ImageSource::Logical(format!("test_assets/{file}.svg"))
}

fn request(source: ImageSource) -> LoadRequest {
    let context = current_context();
    let mut state = context.state.lock().unwrap();
    state.set_source_status(source.clone(), AssetStatus::Pending);
    state.pending_count += 1;
    new_load_request(&mut state, source, true)
}

fn worker() -> (Worker, crossbeam_channel::Receiver<TreeMsg>) {
    let (tree_tx, rx) = bounded(16);
    (
        Worker {
            tree_tx,
            state: current_context().state,
            log_render: false,
        },
        rx,
    )
}

#[test]
fn parsed_completion_after_scene_removal_is_cached_without_resurrecting_source() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let src = source("cache_complex");
    let req = request(src.clone());
    let result = load_source_in_epoch(&src, &req.config, req.epoch, true).unwrap();
    ensure_tree_sources(&ElementTree::new()); // leave before parse result is published
    let (mut worker, rx) = worker();
    worker.complete_load(req, Ok(result.clone()));
    assert!(rx.is_empty());
    assert!(source_status(&src).is_none());
    assert!(asset_record(&result.id).is_some()); // parsed cache, not active source
    assert!(
        !current_context()
            .state
            .lock()
            .unwrap()
            .records
            .contains_key(&result.id)
    );
    ensure_source(&src);
    assert_eq!(source_status(&src), Some(AssetStatus::Ready(result)));
    assert_eq!(svg_cache_stats().parses, 1);
    assert_eq!(svg_cache_stats().rasterizations, 0);
}

#[test]
fn stale_result_cannot_complete_a_new_request_for_the_same_source() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let src = source("cache_complex");
    let old = request(src.clone());
    let result = load_source_in_epoch(&src, &old.config, old.epoch, true).unwrap();
    ensure_tree_sources(&ElementTree::new());
    let new = request(src.clone());
    let (mut worker, rx) = worker();
    worker.complete_load(old, Ok(result.clone()));
    assert!(rx.is_empty());
    assert_eq!(source_status(&src), Some(AssetStatus::Pending));
    worker.handle_load(new);
    assert!(matches!(rx.try_recv(), Ok(TreeMsg::AssetStateChanged)));
    assert_eq!(source_status(&src), Some(AssetStatus::Ready(result)));
    assert_eq!(svg_cache_stats().parses, 1);
}

#[test]
fn configuration_epoch_rejects_late_registration_and_completion() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let src = source("cache_complex");
    let old = request(src.clone());
    let result = load_source_in_epoch(&src, &old.config, old.epoch, true).unwrap();
    let old_epoch = old.epoch;
    runtime.configure(AssetConfig {
        sources: vec!["/not-authorized-anymore".into()],
        ..config()
    });
    let (mut worker, rx) = worker();
    worker.complete_load(old, Ok(result.clone()));
    assert!(rx.is_empty());
    assert!(asset_record(&result.id).is_none());
    assert!(source_status(&src).is_none());
    let tree = usvg::Tree::from_str(
        "<svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'/>",
        &usvg::Options::default(),
    )
    .unwrap();
    assert!(register_vector_asset_in_epoch("late", "late", tree, 1, old_epoch, 1).is_err());
    assert_eq!(svg_cache_stats().entries, 0);
    ensure_source(&src);
    assert_eq!(source_status(&src), Some(AssetStatus::Pending));
    assert!(asset_record(&result.id).is_none());
}

#[test]
fn concurrent_loaders_share_font_discovery_and_parsing() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let context = runtime.context();
            thread::spawn(move || {
                let _guard = context.enter();
                load_source_in_epoch(&source("cache_complex"), &config(), current_epoch(), true)
                    .unwrap()
            })
        })
        .collect();
    let ids: HashSet<_> = threads
        .into_iter()
        .map(|handle| handle.join().unwrap().id)
        .collect();
    assert_eq!(ids.len(), 1);
    assert_eq!(svg_cache_stats().parses, 1);
    assert_eq!(svg_cache_stats().font_discoveries, 1);
}

#[test]
fn parsed_lru_and_estimated_budget_evict_without_duplicate_charges() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(AssetConfig {
        svg_tree_max_entries: 2,
        ..config()
    });
    let (mut worker, _) = worker();
    let load = |worker: &mut Worker, src: ImageSource| {
        worker.handle_load(request(src.clone()));
        let Some(AssetStatus::Ready(asset)) = source_status(&src) else {
            panic!("not ready");
        };
        ensure_tree_sources(&ElementTree::new());
        asset
    };
    let a = load(&mut worker, source("cache_complex"));
    let b = load(&mut worker, source("single_axis_square"));
    let a_record = asset_record(&a.id).unwrap(); // a is now the newer tree
    let a_charge = estimated_tree_bytes(&a_record);
    let c = load(&mut worker, source("single_axis_wide"));
    assert!(asset_record(&b.id).is_none());
    assert!(asset_record(&a.id).is_some());
    assert!(asset_record(&c.id).is_some());
    assert_eq!(svg_cache_stats().entries, 2);
    let before = svg_cache_stats().estimated_bytes;
    load(&mut worker, source("cache_complex"));
    assert_eq!(svg_cache_stats().estimated_bytes, before);
    {
        let context = current_context();
        let mut state = context.state.lock().unwrap();
        state.svg_cache.configure(2, a_charge - 1);
        assert!(state.svg_cache.stats.estimated_bytes < a_charge);
    }
    assert!(asset_record(&a.id).is_none());
    assert_eq!(svg_cache_stats().font_discoveries, 1);
}

#[test]
fn replacing_source_status_releases_unreferenced_active_records() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let src = source("cache_complex");
    let (mut worker, _) = worker();
    worker.handle_load(request(src.clone()));
    let Some(AssetStatus::Ready(asset)) = source_status(&src) else {
        panic!("not ready");
    };
    let context = current_context();
    let mut state = context.state.lock().unwrap();
    state.set_source_status(src, AssetStatus::Failed);
    assert!(!state.records.contains_key(&asset.id));
    assert!(state.svg_cache.get(&asset.id).is_some()); // separate bounded ownership
}

#[test]
fn revalidation_reuses_unchanged_parses_and_publishes_changed_or_deleted_sources() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    let root = std::env::temp_dir().join(format!(
        "emerge-svg-revalidate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let path = root.join("changing.svg");
    fs::write(
        &path,
        "<svg xmlns='http://www.w3.org/2000/svg' width='200' height='200'/>",
    )
    .unwrap();
    runtime.configure(AssetConfig {
        sources: vec![root.display().to_string()],
        ..config()
    });
    let src = ImageSource::Logical("changing.svg".into());
    let (mut worker, _) = worker();
    worker.handle_load(request(src.clone()));
    let Some(AssetStatus::Ready(original)) = source_status(&src) else {
        panic!("not ready");
    };
    let revalidate = || {
        let context = current_context();
        let mut state = context.state.lock().unwrap();
        new_load_request(&mut state, src.clone(), false)
    };
    ensure_tree_sources(&ElementTree::new());
    ensure_source(&src);
    assert_eq!(
        source_status(&src),
        Some(AssetStatus::Ready(original.clone()))
    );
    worker.handle_load(revalidate());
    assert_eq!(svg_cache_stats().parses, 1);
    fs::write(
        &path,
        "<svg xmlns='http://www.w3.org/2000/svg' width='100' height='50'/>",
    )
    .unwrap();
    worker.handle_load(revalidate());
    let Some(AssetStatus::Ready(changed)) = source_status(&src) else {
        panic!("not ready");
    };
    assert_ne!(changed.id, original.id);
    assert_eq!((changed.width, changed.height), (100, 50));
    assert_eq!(svg_cache_stats().parses, 2);
    assert_eq!(svg_cache_stats().font_discoveries, 1);
    assert!(
        !current_context()
            .state
            .lock()
            .unwrap()
            .records
            .contains_key(&original.id)
    );
    fs::remove_file(&path).unwrap();
    worker.handle_load(revalidate());
    assert_eq!(source_status(&src), Some(AssetStatus::Failed));
    assert!(
        !current_context()
            .state
            .lock()
            .unwrap()
            .records
            .contains_key(&changed.id)
    );
}

#[test]
fn hydration_is_deduplicated_and_invalidates_pre_eviction_layer_fingerprints() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    runtime.configure(config());
    let src = source("cache_complex");
    let (mut worker, _) = worker();
    worker.handle_load(request(src.clone()));
    let Some(AssetStatus::Ready(asset)) = source_status(&src) else {
        panic!("not ready");
    };
    let original_revision = asset_record(&asset.id)
        .unwrap()
        .render_revision
        .load(Ordering::Relaxed);
    let context = current_context();
    {
        let mut state = context.state.lock().unwrap();
        state.records.clear();
        state.svg_cache.configure(0, 0);
    }
    let (tx, rx) = bounded(16);
    *context.tx.lock().unwrap() = Some(tx);
    for _ in 0..8 {
        request_asset_hydration(&asset.id);
    }
    assert_eq!(rx.len(), 1);
    let AssetMsg::Load(request) = rx.recv().unwrap() else {
        panic!("not a load");
    };
    worker.handle_load(request);
    assert_ne!(
        asset_record(&asset.id)
            .unwrap()
            .render_revision
            .load(Ordering::Relaxed),
        original_revision
    );
    assert!(asset_record(&asset.id).is_some());
    ensure_tree_sources(&ElementTree::new());
    assert!(asset_record(&asset.id).is_none());
}
