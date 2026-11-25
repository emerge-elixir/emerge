use super::*;
fn settings(entries: usize, bytes: usize) -> Settings {
    Settings {
        svg_tree_max_entries: entries,
        svg_tree_max_bytes: bytes,
        ..Default::default()
    }
}
fn doc(attrs: &str, body: &str) -> String {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {attrs}>{body}</svg>"#)
}
fn load(p: &Pictures, s: &str) -> Option<Art> {
    p.load(AssetKey::Content(digest(s.as_bytes())), s.as_bytes())
}
#[test]
fn intrinsic_units_viewbox_and_bounded_static_effects() {
    let p = Pictures::new(&settings(4, MIB));
    for (axes, size) in [
        (r#"width="0.5in" height="9pt""#, (48., 12.)),
        (r#"viewBox="-4 3 5.25 2.5""#, (5.25, 2.5)),
        (r#"width="42" viewBox="1 1 4 2""#, (42., 21.)),
    ] {
        assert_eq!(
            load(&p, &doc(axes, "<path d='M0 0L2 2'/>")).unwrap().size(),
            size
        );
    }
    let primitive = "<feColorMatrix values='1 0 0 0 0 0 1 0 0 0 0 0 1 0 0 0 0 0 1 0'/>";
    let matrix = |count| {
        doc(
            "width='9' height='8'",
            &format!(
                "<filter id='f'>{}</filter><rect width='9' height='8' filter='url(#f)'/>",
                primitive.repeat(count)
            ),
        )
    };
    let art = load(&p, &matrix(8)).unwrap();
    assert!(art.fits(2., 2.));
    assert!(!art.fits(2000., 2000.));
    assert!(load(&p, &matrix(9)).is_none());
    let depth = |n| {
        doc(
            "viewBox='0 0 8 4'",
            &format!("{}{}", "<g>".repeat(n), "</g>".repeat(n)),
        )
    };
    assert!(load(&p, &depth(63)).is_some());
    assert!(load(&p, &depth(64)).is_none());
    for axes in [
        "",
        "width='20'",
        "width='2em' height='1'",
        "width='0' height='8'",
        "width='100%' height='100%'",
        "width='1' height='32769'",
        "viewBox='0 0 0 16'",
        "viewBox='0 0 1 1' preserveAspectRatio='nope'",
    ] {
        assert!(load(&p, &doc(axes, "")).is_none(), "{axes}");
    }
    for body in [
        "<script/>",
        "<style/>",
        "<animate/>",
        "<image href='a.png'/>",
        "<image href='data:image/png,abc'/>",
        "<use href='else.svg#star'/>",
        "<use href='#absent'/>",
        "<g id='it'><use href='#it'/></g>",
        "<rect style='filter:blur(3px)'/>",
        "<filter><feGaussianBlur/></filter>",
        "<filter><feColorMatrix values='1'/></filter>",
        "<rect fill='url(file:///path)'/>",
        "<rect id='rect'/><g mask='url(#rect)'/>",
        "<pattern id='p'/><use href='#p'/>",
        "<g style='clip-path:url(#g)' id='g'/>",
        "<mask mask-type='unsupported'/>",
        "<filter color-interpolation-filters='unknown'><feColorMatrix/></filter>",
        "<?xml-stylesheet example?>",
    ] {
        assert!(
            load(&p, &doc("viewBox='0 0 8 4'", body)).is_none(),
            "{body}"
        );
    }
    for xml in [
        "<!DOCTYPE svg><svg/>",
        "<html/>",
        "<unclosed",
        "test_assets/check.svg",
    ] {
        assert!(load(&p, xml).is_none());
    }
    assert!(load(&p, &doc("viewBox='0 0 2 2'", &"<g/>".repeat(8192))).is_none());
    assert!(
        load(
            &p,
            &doc(
                "viewBox='0 0 2 2'",
                &format!("<path d='M0 0{}'/>", "L1 1".repeat(policy::WORK))
            )
        )
        .is_none()
    );
    let a = doc(
        "viewBox='0 0 4 4'",
        "<g transform='scale(1000)' opacity='.5'><rect width='40' height='40'/></g>",
    );
    assert!(load(&p, &a).is_none());
}

#[test]
fn cache_limits_apply_to_idle_pictures_not_live_handles_or_other_renderers() {
    let a = doc("viewBox='0 0 1 1'", "<rect width='1' height='1'/>");
    let b = doc("viewBox='0 0 2 1'", "");
    for (entries, bytes) in [(0, MIB), (1, 0), (1, MIB)] {
        let p = Pictures::new(&settings(entries, bytes));
        let bound = p
            .load(AssetKey::Name("a.svg".into()), a.as_bytes())
            .unwrap();
        assert_eq!(load(&p, &a), Some(bound.clone()));
        let other = load(&p, &b).unwrap();
        assert_eq!(p.named("a.svg"), Some(bound.clone())); // weak hit, even after eviction
        assert_eq!(p.counts(), (2, 2));
        let expected = entries != 0 && bytes != 0;
        drop((bound, other));
        assert_eq!(p.named("a.svg").is_some(), expected);
        assert!(p.known(&digest(b.as_bytes())).is_none());
        let _again = load(&p, &a).unwrap();
        let n = if expected { 2 } else { 3 };
        assert_eq!(p.counts(), (n, n));
        let fresh = Pictures::new(&settings(entries, bytes));
        assert!(fresh.named("a.svg").is_none());
        assert!(fresh.known(&digest(a.as_bytes())).is_none());
    }
}
