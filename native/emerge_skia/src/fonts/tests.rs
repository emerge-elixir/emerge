use super::*;

#[test]
fn bundled_inter_faces_decode_and_resolve_with_real_weight_and_slant() {
    let context = crate::assets::AssetContext::default();
    let _guard = context.enter();
    let face_ids: std::collections::HashSet<_> =
        [(400, false), (700, false), (400, true), (700, true)]
            .into_iter()
            .map(|(weight, italic)| {
                let face = get_typeface(&FontKey::new("default", weight, italic))
                    .expect("every bundled Inter face must decode");
                assert_eq!(face.family_name(), "Inter");
                assert_eq!(face.is_bold(), weight == 700);
                assert_eq!(face.is_italic(), italic);
                let (resolved, exact) = resolve_typeface_with_fallback("default", weight, italic);
                assert!(exact, "Inter must not need a synthetic style");
                assert!(Arc::ptr_eq(&face, &resolved));
                face.unique_id()
            })
            .collect();
    assert_eq!(face_ids.len(), 4);
}

#[test]
fn bundled_monospace_faces_are_real_styles_shared_by_all_aliases() {
    let cache = default_font_cache();
    let face_ids: std::collections::HashSet<_> =
        [(400, false), (700, false), (400, true), (700, true)]
            .into_iter()
            .map(|(weight, italic)| {
                let face = cache
                    .get(&FontKey::new("monospace", weight, italic))
                    .expect("bundled JetBrains Mono NL face must decode");
                assert_eq!(face.family_name(), "JetBrains Mono NL");
                assert!(face.is_fixed_pitch());
                assert_eq!(face.is_bold(), weight == 700);
                assert_eq!(face.is_italic(), italic);
                for family in MONOSPACE_FONT_FAMILIES {
                    let alias = &cache[&FontKey::new(*family, weight, italic)];
                    assert!(Arc::ptr_eq(face, alias), "alias {family} duplicates a face");
                }
                face.unique_id()
            })
            .collect();
    assert_eq!(face_ids.len(), 4);

    let default = &cache[&FontKey::default_regular()];
    assert_eq!(default.family_name(), "Inter");
    assert!(!default.is_fixed_pitch());
}

#[test]
fn bundled_monospace_resolves_without_system_fonts_and_has_equal_advances() {
    let context = crate::assets::AssetContext::default();
    let _guard = context.enter();

    for family in MONOSPACE_FONT_FAMILIES {
        for (weight, italic) in [(400, false), (700, false), (400, true), (700, true)] {
            let (face, exact) = resolve_typeface_with_fallback(family, weight, italic);
            assert!(exact);
            assert_eq!(face.family_name(), "JetBrains Mono NL");
            let font = make_font_with_style(family, weight, italic, 22.0);
            let advance = font.measure_str("i", None).0;
            assert!(advance > 0.0);
            for text in ["W", "0", ".", " "] {
                assert!(
                    (font.measure_str(text, None).0 - advance).abs() < 0.001,
                    "{family} weight={weight} italic={italic}: {text:?} must use a fixed advance"
                );
            }
        }
    }
}
