use super::*;
use crate::renderer::ImageSnapshot;
use crate::tree::element::NodeId;

fn raster(id: &str, width: u32, height: u32) {
    crate::renderer::insert_test_raster_asset_rgba(
        id,
        width,
        height,
        &[255, 0, 0, 255].repeat((width * height) as usize),
    )
    .unwrap();
}
fn ready(id: &str, width: u32, height: u32) -> AssetStatus {
    AssetStatus::Ready(ResolvedAsset {
        id: id.into(),
        width,
        height,
    })
}
// Independent pre-replacement map reconstruction. Do not share the in-place
// normalizer with this reference: it owns/replaces values exactly as before.
fn reference(
    statuses: HashMap<ImageSource, AssetStatus>,
    images: &ImageSnapshot,
) -> HashMap<ImageSource, AssetStatus> {
    statuses
        .into_iter()
        .map(|(source, status)| {
            let id = match &source {
                ImageSource::Id(id) => Some(id),
                _ => match &status {
                    AssetStatus::Ready(asset) => Some(&asset.id),
                    _ => None,
                },
            };
            let status = id
                .and_then(|id| {
                    images.dimensions(id).map(|(width, height)| {
                        AssetStatus::Ready(ResolvedAsset {
                            id: id.clone(),
                            width,
                            height,
                        })
                    })
                })
                .unwrap_or(status);
            (source, status)
        })
        .collect()
}
#[test]
fn in_place_status_capture_matches_independent_map_reconstruction() {
    for width in [0, 1, 8, 64, 512] {
        let runtime = AssetRuntime::new();
        let _guard = runtime.enter();
        runtime.configure(AssetConfig {
            cache_max_entries: 4096,
            ..AssetConfig::default()
        });
        let entries = (0..width)
            .map(|i| {
                let id = format!("asset/{i}/{}", "雪".repeat(16));
                let size = (i as u32 % 4 + 1, i as u32 % 3 + 1);
                let (source, status, binding) = match i % 10 {
                    0 => (ImageSource::Id(id.clone()), ready(&id, 99, 88), true),
                    1 => (ImageSource::Id(id.clone()), AssetStatus::Pending, true),
                    2 => (ImageSource::Id(id.clone()), AssetStatus::Failed, true),
                    3 => (ImageSource::Id(id.clone()), ready("wrong-id", 99, 88), true),
                    4 => (
                        ImageSource::Logical(format!("logical/{i}")),
                        ready(&id, 99, 88),
                        true,
                    ),
                    5 => (
                        ImageSource::RuntimePath(format!("runtime/{i}")),
                        ready(&id, 99, 88),
                        true,
                    ),
                    6 => (
                        ImageSource::Logical(format!("pending/{i}")),
                        AssetStatus::Pending,
                        false,
                    ),
                    7 => (
                        ImageSource::RuntimePath(format!("failed/{i}")),
                        AssetStatus::Failed,
                        false,
                    ),
                    8 => (
                        ImageSource::Logical(format!("missing/{i}")),
                        ready(&id, 99, 88),
                        false,
                    ),
                    _ => (
                        ImageSource::Id(id.clone()),
                        ready("unbound-other-id", 99, 88),
                        false,
                    ),
                };
                if binding {
                    raster(&id, size.0, size.1);
                }
                (source, status, binding.then_some(size))
            })
            .collect::<Vec<_>>();
        let expected_loading = {
            let context = current_context();
            let mut state = context.state.lock().unwrap();
            entries
                .iter()
                .enumerate()
                .for_each(|(i, (source, status, _))| {
                    state.set_source_status(source.clone(), status.clone());
                    if matches!(status, AssetStatus::Pending) && i % 2 == 0 {
                        state.loading_indicators.insert(source.clone(), None);
                    }
                });
            state
                .loading_indicators
                .iter()
                .filter(|(_, deadline)| deadline.is_none())
                .map(|(source, _)| source.clone())
                .collect::<HashSet<_>>()
        };
        let sources = FrameSourceList {
            all: entries
                .iter()
                .map(|(source, _, _)| source.clone())
                .collect(),
            measured: entries
                .iter()
                .enumerate()
                .map(|(i, (source, _, _))| (NodeId::from_u64(i as u64 + 1), source.clone()))
                .collect(),
        };
        let old_input = entries
            .iter()
            .map(|(source, status, _)| (source.clone(), status.clone()))
            .collect();
        let frame = FrameAssets::capture(&sources);
        let expected = reference(old_input, &frame.images);
        assert_eq!(frame.sources, expected);
        assert_eq!(frame.loading_indicators, expected_loading);
        for (i, (source, old, size)) in entries.iter().enumerate() {
            let literal = if let Some((width, height)) = size {
                let id = match source {
                    ImageSource::Id(id) => id,
                    _ => match old {
                        AssetStatus::Ready(asset) => &asset.id,
                        _ => unreachable!(),
                    },
                };
                ready(id, *width, *height)
            } else {
                old.clone()
            };
            assert_eq!(frame.sources.get(source), Some(&literal));
            let measured = match literal {
                AssetStatus::Ready(asset) => Some((asset.width, asset.height)),
                _ => None,
            };
            assert_eq!(
                frame.measured.get(&NodeId::from_u64(i as u64 + 1)),
                Some(&measured)
            );
            assert_eq!(
                source_status(source),
                Some(old.clone()),
                "capture must not update runtime status"
            );
        }
    }
}
#[test]
fn ready_normalization_keeps_owned_id_buffers_and_uses_frozen_dimensions() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    let id = "asset/雪".repeat(1024);
    raster(&id, 2, 3);
    let frame = FrameAssets::capture(&FrameSourceList {
        all: HashSet::from([ImageSource::Id(id.clone())]),
        ..FrameSourceList::default()
    });
    raster(&id, 4, 5);
    assert_eq!(frame.images.dimensions(&id), Some((2, 3)));
    assert_eq!(crate::renderer::asset_dimensions(&id), Some((4, 5)));
    for source in [
        ImageSource::Id(id.clone()),
        ImageSource::Logical("alias".into()),
        ImageSource::RuntimePath("alias-path".into()),
    ] {
        let mut status = ready(&id, 99, 88);
        let original = match &status {
            AssetStatus::Ready(asset) => asset.id.as_ptr(),
            _ => unreachable!(),
        };
        let expected = reference(
            HashMap::from([(source.clone(), status.clone())]),
            &frame.images,
        );
        resolve_snapshot_dimensions(&source, &mut status, &frame.images);
        assert_eq!(Some(&status), expected.get(&source));
        let AssetStatus::Ready(asset) = &status else {
            panic!("ready")
        };
        assert_eq!(asset.id.as_ptr(), original);
        assert_eq!((asset.width, asset.height), (2, 3));
    }
}
#[test]
fn stop_capture_does_not_normalize_missing_new_epoch_bindings_from_old_pins() {
    let runtime = AssetRuntime::new();
    let _guard = runtime.enter();
    let source = ImageSource::Id("epoch-image".into());
    raster("epoch-image", 2, 3);
    let sources = FrameSourceList {
        all: HashSet::from([source.clone()]),
        measured: vec![(NodeId(1), source.clone())],
    };
    let old = FrameAssets::capture(&sources);
    assert_eq!(old.sources.get(&source), Some(&ready("epoch-image", 2, 3)));
    runtime.configure(AssetConfig {
        decode_at_size: true,
        ..AssetConfig::default()
    });
    // Reconfiguration intentionally keeps cached raster bindings. Stop is the
    // full renderer-cache/epoch reset, not a different normalization policy.
    let reconfigured = FrameAssets::capture(&sources);
    assert_eq!(reconfigured.images.dimensions("epoch-image"), Some((2, 3)));
    assert_eq!(
        reconfigured.sources.get(&source),
        Some(&ready("epoch-image", 2, 3))
    );
    runtime.stop();
    let new = FrameAssets::capture(&sources);
    assert_eq!(new.images.dimensions("epoch-image"), None);
    assert_eq!(new.sources.get(&source), Some(&AssetStatus::Pending));
    assert_eq!(new.measured.get(&NodeId(1)), Some(&None));
    assert_eq!(old.images.dimensions("epoch-image"), Some((2, 3)));
    assert_eq!(old.sources.get(&source), Some(&ready("epoch-image", 2, 3)));
}
