use super::*;
use std::{fs, os::unix::fs::symlink, path::PathBuf};

struct Temp(PathBuf);
impl Temp {
    fn new(label: &str) -> Self {
        // TMPDIR itself may be a symlink (e.g. /var on macOS). The exact-file
        // controls must test the grant, not accidentally refuse its parent.
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("emerge-{label}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn loader(&self) -> Loader {
        Loader::new(Settings {
            sources: vec![self.0.to_str().unwrap().into()],
            decode_at_size: false,
            cache_max_entries: 1,
            cache_max_bytes: MAX_BITMAP,
            ..Default::default()
        })
        .unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../priv/test_assets")
}

#[test]
fn directory_capability_file_and_decode_limits_precede_pixel_allocation() {
    let dir = Temp::new("png");
    let root = &dir.0;
    let priv_dir = fixtures();
    let wide = fs::read(priv_dir.join("single_axis_wide.png")).unwrap();
    fs::write(root.join("wide.png"), &wide).unwrap();
    fs::copy(priv_dir.join("alpha_grid.png"), root.join("grid.png")).unwrap();
    symlink(&priv_dir, root.join("link")).unwrap();
    symlink(priv_dir.join("alpha_grid.png"), root.join("link.png")).unwrap();
    let mut bad = wide.clone();
    bad[16..20].copy_from_slice(&32768u32.to_be_bytes());
    bad[20..24].copy_from_slice(&32768u32.to_be_bytes());
    fs::write(root.join("huge.png"), bad).unwrap();
    fs::write(root.join("bad.png"), &wide[..100]).unwrap();
    let mut corrupt = wide.clone();
    let idat = corrupt.windows(4).position(|tag| tag == b"IDAT").unwrap();
    corrupt[idat + 4] ^= 0xff;
    fs::write(root.join("corrupt.png"), corrupt).unwrap();
    File::create(root.join("large-file.png"))
        .unwrap()
        .set_len(MAX_FILE as u64 + 1)
        .unwrap();
    let mut l = dir.loader();
    for name in [
        "../other.png",
        "/abs.png",
        "no.png",
        "link.png",
        "link/alpha_grid.png",
        "huge.png",
        "bad.png",
        "corrupt.png",
        "large-file.png",
    ] {
        assert_eq!(l.load(name.into()), Source::Failed);
    }
    assert_eq!(l.resident.load(Ordering::Relaxed), 0);
    let a = l.load("wide.png".into());
    assert_eq!(a.dimensions(), Some((200., 100.)));
    assert_eq!(a, l.load("wide.png".into()));
    let b = l.load("grid.png".into());
    assert_eq!(l.cache.len(), 1);
    assert_eq!(l.resident.load(Ordering::Relaxed), 80_240);
    drop(a);
    assert_eq!(l.resident.load(Ordering::Relaxed), 240);
    assert!(l.evict());
    assert_eq!(b.dimensions(), Some((10., 6.)));
    drop(b);
    assert_eq!(l.resident.load(Ordering::Relaxed), 0);
}

#[test]
fn log_gauges_distinguish_resident_from_cached_without_keeping_images_alive() {
    let mut l = Loader::new(Settings {
        sources: vec![fixtures().display().to_string()],
        cache_max_entries: 1,
        cache_max_bytes: MIB,
        svg_tree_max_entries: 1,
        svg_tree_max_bytes: MIB,
        ..Settings::default()
    })
    .unwrap();
    let grid = l.load("alpha_grid.png".into());
    let gauges = l.usage();
    let art = l.load("single_axis_square.svg".into());
    assert!(matches!(art, Source::Picture(_)));
    let active = gauges.to_string();
    assert!(active.contains("raster cache: entries=1 bytes=240"));
    assert!(active.contains("SVG pictures: cached_entries=1"));
    assert!(l.evict());
    let pinned = gauges.to_string();
    assert!(pinned.contains("raster cache: entries=0 bytes=0"));
    assert!(pinned.contains("live charged raster bytes=240"));
    drop((grid, art, l));
    let released = gauges.to_string();
    assert!(released.contains("live charged raster bytes=0"));
    assert!(released.contains("SVG pictures: catalog unavailable"));
}

#[test]
fn runtime_cache_hits_require_the_declared_decoder_and_current_file_capability() {
    let dir = Temp::new("content-reader");
    for (asset, ext, dimensions) in [
        ("alpha_grid.png", "png", (10., 6.)),
        ("stills/indexed4.bmp", "BMP", (16., 8.)),
        ("stills/turn.webp", "WebP", (6., 10.)),
    ] {
        let data = fs::read(fixtures().join(asset)).unwrap();
        let bytes = InputBudget::default().copy(&data).unwrap();
        let wrong = dir.0.join("wrong.jpg");
        let name = dir.0.join(format!("photo.{ext}"));
        let denied = dir.0.join(format!("denied.{ext}"));
        for p in [&name, &wrong, &denied] {
            fs::write(p, &data).unwrap();
        }
        for sized in [false, true] {
            let mut l = Loader::new(Settings {
                sources: vec![dir.0.display().to_string()],
                decode_at_size: sized,
                cache_max_entries: 2,
                cache_max_bytes: MIB,
                runtime_enabled: true,
                follow_symlinks: true,
                allowlist: vec![name.display().to_string(), wrong.display().to_string()],
                extensions: vec![format!(".{}", ext.to_ascii_lowercase()), ".jpg".into()],
                max_file_size: MIB,
                ..Default::default()
            })
            .unwrap();
            let input = || Input::Bytes(digest(&data), bytes.clone());
            let ready = l.open_input(input(), false).unwrap();
            assert_eq!(ready.dimensions(), Some(dimensions));
            assert!(l.open_input(input(), true).is_none());
            assert_eq!(
                l.open_input(Input::Runtime(name.display().to_string()), false),
                Some(ready)
            );
            for bad in [&wrong, &denied] {
                assert!(
                    l.open_input(Input::Runtime(bad.display().to_string()), false)
                        .is_none()
                );
            }
        }
    }
}

#[test]
fn logical_svg_aliases_share_a_live_snapshot_only_within_the_same_root() {
    let [a, b] = [Temp::new("svg-root-a"), Temp::new("svg-root-b")];
    let wide = fs::read(fixtures().join("single_axis_wide.svg")).unwrap();
    let square = fs::read(fixtures().join("single_axis_square.svg")).unwrap();
    fs::write(a.0.join("same.svg"), &wide).unwrap();
    fs::write(a.0.join("alias.svg"), &wide).unwrap();
    fs::write(b.0.join("same.svg"), &square).unwrap();
    let absolute = a.0.join("alias.svg").display().to_string();
    let mut first = Loader::new(Settings {
        sources: vec![a.0.display().to_string()],
        runtime_enabled: true,
        allowlist: vec![absolute.clone()],
        extensions: vec![".svg".into()],
        max_file_size: MIB,
        ..Default::default() // no idle retention
    })
    .unwrap();
    let mut second = b.loader();
    let live = first.load("same.svg".into());
    let alias = first.load("alias.svg".into());
    assert_eq!(alias, live);
    let other = second.load("same.svg".into());
    assert_eq!(live.dimensions(), Some((200., 100.)));
    assert_eq!(other.dimensions(), Some((200., 200.)));
    for _ in 0..2 {
        let copy = InputBudget::default().copy(&wide).unwrap();
        assert_eq!(
            first.open_input(Input::Bytes(digest(&wide), copy), true),
            Some(live.clone())
        );
    }
    let runtime = || Input::Runtime(absolute.clone());
    assert_eq!(first.open_input(runtime(), true), Some(live.clone()));
    // The exact-file grant pins the original descriptor. New identical bytes
    // at the very same filename are not a grant, even with the picture live.
    fs::rename(&absolute, a.0.join("original.svg")).unwrap();
    fs::write(&absolute, &wide).unwrap();
    assert!(first.open_input(runtime(), true).is_none());
    fs::write(a.0.join("same.svg"), square).unwrap();
    assert_eq!(first.load("same.svg".into()), live);
    assert_eq!(first.pictures.counts(), (1, 1));
    drop((alias, live));
    let reopened = first.load("same.svg".into());
    assert_eq!(reopened.dimensions(), other.dimensions());
    assert_ne!(reopened, other, "renderers do not share a picture catalog");
    assert_eq!(first.pictures.counts(), (2, 2));
    assert_eq!(second.pictures.counts(), (1, 1));
}

// Change just the SHORT orientation value in our generated little-endian IFD0.
fn jpeg(origin: u8) -> Vec<u8> {
    let mut bytes = fs::read(fixtures().join("turn_right.jpeg")).unwrap();
    assert_eq!(&bytes[22..32], &[0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0]);
    bytes[30] = origin;
    bytes
}
fn record(l: &mut Loader, name: &str) -> Record {
    let Source::Sized(r) = l.load(name.into()) else {
        panic!("no metadata for {name}")
    };
    r
}
fn fitted(l: &mut Loader, r: &Record, size: (i32, i32)) -> BitmapRef {
    let mut got = None;
    assert!(l.fulfill(Request::Fit(r.clone(), size.into()), |s| {
        let Source::Ready(b) = s else { return false };
        r.publish(&b);
        got = Some(b);
        true
    }));
    got.unwrap()
}
fn ready(l: &mut Loader, name: &str) -> BitmapRef {
    let Source::Ready(bitmap) = l.load(name.into()) else {
        panic!("failed to load {name}")
    };
    bitmap
}
#[test]
fn all_exif_origins_publish_the_oriented_size_and_every_oriented_texel() {
    let dir = Temp::new("jpeg-origins");
    let mut l = dir.loader();
    fs::write(dir.0.join("raw.JPG"), jpeg(1)).unwrap();
    let raw = ready(&mut l, "raw.JPG");
    let raw = raw.image().peek_pixels().unwrap();
    let (w, h) = (16, 8);
    assert_eq!((raw.width(), raw.height()), (w, h));
    for n in 1..=8 {
        let name = format!("{n}.jPeG");
        fs::write(dir.0.join(&name), jpeg(n)).unwrap();
        let img = ready(&mut l, &name);
        let got = img.image().peek_pixels().unwrap();
        assert_eq!(
            (got.width(), got.height()),
            if n < 5 { (w, h) } else { (h, w) }
        );
        for y in 0..got.height() {
            for x in 0..got.width() {
                let p = match n {
                    1 => (x, y),
                    2 => (w - 1 - x, y),
                    3 => (w - 1 - x, h - 1 - y),
                    4 => (x, h - 1 - y),
                    5 => (y, x),
                    6 => (y, h - 1 - x),
                    7 => (w - 1 - y, h - 1 - x),
                    8 => (w - 1 - y, x),
                    _ => unreachable!(),
                };
                assert_eq!(
                    got.get_color((x, y)),
                    raw.get_color(p),
                    "origin {n} at ({x},{y})"
                );
            }
        }
    }
}
// Three visible pixels per row; the last nibble/bits and the padding are
// intentionally nonzero. RGBQUAD's fourth byte is reserved, not transparency.
fn indexed_bmp(bpp: u8, colors: usize, rows: [[u8; 4]; 2]) -> Vec<u8> {
    let offset = 54 + 4 * colors;
    let mut b = vec![0; offset];
    b[..2].copy_from_slice(b"BM");
    for (at, value) in [
        (2, (offset + 8) as u32),
        (10, offset as u32),
        (14, 40),
        (18, 3),
        (22, (-2i32) as u32),
        (46, colors as u32),
    ] {
        b[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    b[26] = 1;
    b[28] = bpp;
    for (i, color) in b[54..].chunks_exact_mut(4).enumerate() {
        color.copy_from_slice(&[0, 0, i as u8, 7]);
    }
    b.extend(rows.into_iter().flatten());
    b
}
#[test]
fn indexed_bmp_expands_only_visible_bits_and_rejects_invalid_tables_and_offsets() {
    let dir = Temp::new("indexed-bmp");
    let mut l = dir.loader();
    for (bits, colors, rows, want) in [
        (
            1,
            2,
            [[0xbf, 255, 255, 255], [0x5f, 255, 255, 255]],
            [1, 0, 1, 0, 1, 0],
        ),
        (
            4,
            3,
            [[0x20, 0x1f, 255, 255], [0x12, 0x0f, 255, 255]],
            [2, 0, 1, 1, 2, 0],
        ),
        (
            8,
            249,
            [[248, 17, 2, 255], [0, 243, 1, 255]],
            [248, 17, 2, 0, 243, 1],
        ),
    ] {
        let b = indexed_bmp(bits, colors, rows);
        let name = format!("odd{bits}.bmp");
        fs::write(dir.0.join(&name), &b).unwrap();
        let bmp = ready(&mut l, &name);
        let px = bmp.image().peek_pixels().unwrap();
        for (i, r) in want.into_iter().enumerate() {
            assert_eq!(
                px.get_color(((i % 3) as i32, (i / 3) as i32)),
                skia_safe::Color::from_rgb(r, 0, 0)
            );
        }
        for (i, val) in [
            (10, 54),
            (14, 108),
            (26, 2),
            (28, 16),
            (30, 1),
            (30, 3),
            (34, 7),
            (46, 257),
        ] {
            let mut bad = b.clone();
            bad[i] = val as u8;
            if val > 255 {
                bad[i + 1] = 1;
            }
            assert!(Format::Bmp.codec(Data::new_copy(&bad)).is_none());
        }
        assert!(
            Format::Bmp
                .codec(Data::new_copy(&b[..b.len() - 1]))
                .is_none()
        );
        if bits > 1 {
            let mut bad = b.clone();
            bad[b.len() - 8] = 255;
            assert!(Format::Bmp.codec(Data::new_copy(&bad)).is_none());
        }
    }
}

#[test]
fn webp_preflight_bounds_chunks_and_rejects_inconsistent_stills_before_pixel_work() {
    let fixtures = fixtures().join("stills");
    let extended = fs::read(fixtures.join("alpha.webp")).unwrap();
    let lossless = fs::read(fixtures.join("lossless.webp")).unwrap();
    let reject = |data: &[u8]| assert!(Format::Webp.codec(Data::new_copy(data)).is_none());
    reject(&fs::read(fixtures.join("animated.webp")).unwrap());
    reject(&extended[..extended.len() - 1]);
    let mut extra = lossless.clone();
    extra.extend_from_slice(&lossless[12..]);
    let len = (extra.len() - 8) as u32;
    extra[4..8].copy_from_slice(&len.to_le_bytes());
    reject(&extra);
    for (at, value) in [
        (4, 0),
        (16, 255),
        (20, 18),
        (20, 128),
        (20, 0),
        (21, 1),
        (24, 99),
        (26, 254),
        (34, 255),
        (38, 255),
    ] {
        let mut bad = extended.clone();
        bad[at] = value;
        reject(&bad);
    }
    let mut bad = extended.clone();
    bad[30..34].copy_from_slice(b"ANMF");
    reject(&bad);
    let mut bad = lossless.clone();
    bad[16..20].copy_from_slice(&25u32.to_le_bytes());
    *bad.last_mut().unwrap() = 1;
    reject(&bad);

    let mut metadata = extended.clone();
    metadata[20] |= 4;
    metadata.extend_from_slice(b"XMP ");
    metadata.extend_from_slice(&(MIB as u32 + 2).to_le_bytes());
    metadata.resize(metadata.len() + MIB + 2, b' ');
    let len = (metadata.len() - 8) as u32;
    metadata[4..8].copy_from_slice(&len.to_le_bytes());
    reject(&metadata);
    let mut huge = lossless.clone();
    huge[21..25].copy_from_slice(&0x0fff_ffffu32.to_le_bytes());
    reject(&huge);

    // A RIFF with a complete VP8L dimension header but not its entropy stream
    // opens metadata but must never publish pixels.
    let dir = Temp::new("short-vp8l");
    let mut short = lossless[..28].to_vec();
    short[4..8].copy_from_slice(&20u32.to_le_bytes());
    short[16..20].copy_from_slice(&8u32.to_le_bytes());
    fs::write(dir.0.join("bad.webp"), &short).unwrap();
    let mut l = dir.loader();
    assert_eq!(l.load("bad.webp".into()), Source::Failed);
    l.settings.decode_at_size = true;
    let r = record(&mut l, "bad.webp");
    assert_eq!(r.size(), (16, 8).into());
    assert!(l.fit(&r, (4, 2).into()).is_none());
}

#[test]
fn fitted_webp_orients_the_codec_output_exactly_once() {
    let dir = Temp::new("turn-webp");
    let mut l = dir.loader();
    l.settings.decode_at_size = true;
    for name in ["clear.webp", "turn.webp"] {
        fs::copy(fixtures().join("stills").join(name), dir.0.join(name)).unwrap();
    }
    let raw = record(&mut l, "clear.webp");
    let turn = record(&mut l, "turn.webp");
    assert_eq!(turn.size(), (6, 10).into());
    for (w, h) in [(5, 3), (10, 6)] {
        let a = fitted(&mut l, &raw, (w, h));
        let b = fitted(&mut l, &turn, (h, w));
        let a = a.image().peek_pixels().unwrap();
        let b = b.image().peek_pixels().unwrap();
        for y in 0..w {
            for x in 0..h {
                assert_eq!(b.get_color((x, y)), a.get_color((y, h - 1 - x)));
            }
        }
    }
}

#[test]
fn jpeg_requires_a_complete_matching_file_and_bounded_dimensions() {
    let dir = Temp::new("jpeg-files");
    let root = &dir.0;
    let mut l = dir.loader();
    let photo = fs::read(fixtures().join("../sample_assets/static.jpg")).unwrap();
    // This progressive stream already has complete scans. An EOI in APP data
    // must not make the truncated primary image look complete.
    let no_end = [
        &photo[..2],
        b"\xff\xe1\0\x04\xff\xd9",
        &photo[2..photo.len() - 2],
    ]
    .concat();
    fs::write(root.join("truncated.jpg"), no_end).unwrap();
    fs::write(
        root.join("padded.jpg"),
        [&photo[..], b"trailing bytes"].concat(),
    )
    .unwrap();
    fs::write(root.join("invalid.jpg"), b"\xff\xd8\xff\xe1\0\x01\xff\xd9").unwrap();
    fs::write(root.join("invalid.png"), &photo).unwrap();
    fs::copy(fixtures().join("alpha_grid.png"), root.join("wrong.jpeg")).unwrap();
    symlink(fixtures().join("turn_right.jpeg"), root.join("link.jpg")).unwrap();
    symlink(fixtures(), root.join("link")).unwrap();
    fs::create_dir(root.join("dir.jpg")).unwrap();
    File::create(root.join("large.jpg"))
        .unwrap()
        .set_len(MAX_FILE as u64 + 1)
        .unwrap();
    for (name, w, h) in [("wide.jpg", 32769u16, 1u16), ("bitmap.jpg", 4097, 4096)] {
        let mut bytes = jpeg(1);
        let sof = bytes.windows(2).position(|v| v == b"\xff\xc0").unwrap();
        bytes[sof + 5..sof + 7].copy_from_slice(&h.to_be_bytes());
        bytes[sof + 7..sof + 9].copy_from_slice(&w.to_be_bytes());
        fs::write(root.join(name), bytes).unwrap();
    }
    for bad in [
        "truncated.jpg",
        "invalid.jpg",
        "invalid.png",
        "wrong.jpeg",
        "link.jpg",
        "link/turn_right.jpeg",
        "dir.jpg",
        "missing.jpeg",
        "large.jpg",
        "wide.jpg",
        "bitmap.jpg",
    ] {
        assert_eq!(l.load(bad.into()), Source::Failed, "{bad}");
        assert_eq!(l.resident.load(Ordering::Relaxed), 0);
    }
    assert_eq!(l.load("padded.jpg".into()).dimensions(), Some((640., 420.)));
}
#[test]
fn fitted_jpeg_retains_only_fitted_bytes_and_grows_from_the_same_compressed_source() {
    let dir = Temp::new("fitted-jpeg");
    let path = dir.0.join("photo.jpg");
    let encoded = fs::copy(fixtures().join("../sample_assets/static.jpg"), &path).unwrap() as usize;
    let mut l = dir.loader();
    l.settings.decode_at_size = true;
    let r = record(&mut l, "photo.jpg");
    assert_eq!(r.size(), (640, 420).into());
    assert_eq!(l.encoded_bytes.load(Ordering::Relaxed), encoded);
    assert_eq!(l.resident.load(Ordering::Relaxed), 0);
    fs::remove_file(path).unwrap();
    let small = fitted(&mut l, &r, (80, 53));
    assert_eq!(small.image().dimensions(), (80, 53).into());
    assert_eq!(
        small.image().peek_pixels().unwrap().compute_byte_size(),
        16_960
    );
    assert_eq!(l.cached_bytes, 16_960);
    assert_eq!(l.resident.load(Ordering::Relaxed), 16_960);
    let big = fitted(&mut l, &r, (160, 105));
    assert_eq!(
        big.image().peek_pixels().unwrap().compute_byte_size(),
        67_200
    );
    assert_eq!(l.cache.len(), 1);
    assert_eq!(l.cached_bytes, 67_200);
    assert_eq!(
        l.resident.load(Ordering::Relaxed),
        16_960 + 67_200,
        "old frame still pinned"
    );
    drop(small);
    assert_eq!(l.resident.load(Ordering::Relaxed), 67_200);
    assert_eq!(fitted(&mut l, &r, (80, 53)), big);
    assert_eq!(
        l.decodes,
        [
            ((80, 53).into(), (80, 53).into()),
            ((160, 105).into(), (160, 105).into())
        ]
    );
    assert!(l.evict());
    assert_eq!(l.cached_bytes, 0);
    assert_eq!(record(&mut l, "photo.jpg"), r);
    assert_eq!(r.current((90, 53).into()), Some(big.clone()));
    assert!(
        r.current((80, 106).into()).is_none(),
        "height must suffice too"
    );
    drop(big);
    assert_eq!(l.resident.load(Ordering::Relaxed), 0);
    drop(r);
    assert_eq!(l.encoded_bytes.load(Ordering::Relaxed), 0);
}

#[test]
fn fitted_cache_evicts_the_least_recent_draw_and_honors_both_limits() {
    let dir = Temp::new("fitted-lru");
    let mut l = dir.loader();
    l.settings.decode_at_size = true;
    l.settings.cache_max_entries = 2;
    l.settings.cache_max_bytes = 256;
    let [a, b, c] = ["a.png", "b.png", "c.png"].map(|name| {
        fs::copy(fixtures().join("alpha_grid.png"), dir.0.join(name)).unwrap();
        record(&mut l, name)
    });
    let small = (5, 3);
    drop(fitted(&mut l, &a, small));
    drop(fitted(&mut l, &b, small));
    assert!(a.current(small.into()).is_some());
    drop(fitted(&mut l, &c, small));
    assert!(b.current(small.into()).is_none());
    assert_eq!(l.cached_bytes, 120);
    // A rejected larger candidate is not published by the loader.
    assert!(!l.fulfill(Request::Fit(a.clone(), (10, 6).into()), |_| false));
    assert_eq!(l.resident.load(Ordering::Relaxed), 120);
    assert!(a.current((10, 6).into()).is_none());
    drop(fitted(&mut l, &a, (10, 6)));
    assert_eq!(l.cache.len(), 1);
    assert_eq!(l.cached_bytes, 240);
    assert!(c.current(small.into()).is_none());
    assert_eq!(l.resident.load(Ordering::Relaxed), 240);
}

#[test]
fn png_full_staging_is_budgeted_but_only_the_exact_resample_survives() {
    let dir = Temp::new("fitted-png");
    fs::copy(
        fixtures().join("single_axis_wide.png"),
        dir.0.join("wide.png"),
    )
    .unwrap();
    let mut l = dir.loader();
    l.settings.decode_at_size = true;
    l.settings.cache_max_entries = 0;
    let r = record(&mut l, "wide.png");
    let target = (31, 16);
    let bytes = 31 * 16 * 4;
    let pinned = l.charge(MAX_RESIDENT - 80_000 - bytes + 1).unwrap();
    assert!(
        l.fit(&r, target.into()).is_none(),
        "full PNG staging + output must both fit"
    );
    drop(pinned);
    let b = fitted(&mut l, &r, target);
    assert_eq!(l.decodes, [((200, 100).into(), target.into())]);
    assert_eq!(b.image().peek_pixels().unwrap().compute_byte_size(), bytes);
    assert_eq!(l.resident.load(Ordering::Relaxed), bytes);
    assert_eq!(l.cached_bytes, 0);
    drop(b);
    assert!(r.current(target.into()).is_none());
    assert_eq!(l.resident.load(Ordering::Relaxed), 0);
}

#[test]
fn fitted_quarter_turn_uses_the_raw_codec_axes_then_displays_the_rotated_pixels() {
    let dir = Temp::new("fitted-quarter-turn");
    fs::write(dir.0.join("raw.jpg"), jpeg(1)).unwrap();
    fs::write(dir.0.join("turn.jpg"), jpeg(6)).unwrap();
    let mut l = dir.loader();
    l.settings.decode_at_size = true;
    let upright = record(&mut l, "raw.jpg");
    let turned = record(&mut l, "turn.jpg");
    let raw = fitted(&mut l, &upright, (8, 4));
    let turn = fitted(&mut l, &turned, (4, 8));
    assert_eq!(turned.size(), (8, 16).into());
    assert_eq!(
        l.decodes,
        [
            ((8, 4).into(), (8, 4).into()),
            ((8, 4).into(), (4, 8).into())
        ]
    );
    let raw = raw.image().peek_pixels().unwrap();
    let display = turn.image().peek_pixels().unwrap();
    for y in 0..8 {
        for x in 0..4 {
            assert_eq!(display.get_color((x, y)), raw.get_color((y, 3 - x)));
        }
    }
    // An in-between request rounds UP to a supported codec size, then down to the exact target.
    let turn = fitted(&mut l, &turned, (5, 9));
    assert_eq!(turn.image().dimensions(), (5, 9).into());
    assert_eq!(l.decodes.last(), Some(&((10, 5).into(), (5, 9).into())));
    assert_eq!(l.cached_bytes, 5 * 9 * 4);
}

#[test]
fn jpeg_orientation_scratch_competes_with_live_decoded_pixels_but_is_not_cached() {
    let dir = Temp::new("jpeg-budget");
    fs::write(dir.0.join("turn.jpeg"), jpeg(6)).unwrap();
    fs::write(dir.0.join("raw.jpg"), jpeg(1)).unwrap();
    let mut l = dir.loader();
    let bytes = 16 * 8 * 4;
    let pinned = l.charge(MAX_RESIDENT - bytes).unwrap();
    assert_eq!(l.load("turn.jpeg".into()), Source::Failed);
    assert_eq!(l.resident.load(Ordering::Relaxed), MAX_RESIDENT - bytes);
    let raw = ready(&mut l, "raw.jpg");
    assert_eq!(l.resident.load(Ordering::Relaxed), MAX_RESIDENT);
    drop(pinned);
    let turn = ready(&mut l, "turn.jpeg");
    assert_eq!(turn, ready(&mut l, "turn.jpeg"));
    assert_eq!(l.cached_bytes, bytes);
    assert_eq!(l.resident.load(Ordering::Relaxed), 2 * bytes);
    assert!(l.evict());
    drop((raw, turn));
    assert_eq!(l.resident.load(Ordering::Relaxed), 0);
}
