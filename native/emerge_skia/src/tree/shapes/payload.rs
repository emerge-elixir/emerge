use super::*;

/// A borrowed, typed view of the mount's existing storage. No payload is copied
/// into the shape or used as a second child order.
pub(super) trait Payload {
    type Data<'a>;
    const MAX_CHILDREN: usize;
    const USES_GLYPHS: bool = false;
    fn get(e: &Elements, k: Key) -> Self::Data<'_>;

    /// The shared codec has already handled the common box and listener fields.
    fn decode(tag: u8, _: &mut wire::Reader<'_>, _: &[u8], _: &mut Attrs) -> Result<(), String> {
        Err(format!("unsupported attribute {tag} in first slice"))
    }
}

pub(super) struct Children;
impl Payload for Children {
    type Data<'a> = &'a [Key];
    const MAX_CHILDREN: usize = usize::MAX;
    fn get(e: &Elements, k: Key) -> &[Key] {
        e.children(k)
    }
}

pub(super) struct OnlyChild;
impl Payload for OnlyChild {
    type Data<'a> = Option<Key>;
    const MAX_CHILDREN: usize = 1;
    fn get(e: &Elements, k: Key) -> Option<Key> {
        e.children(k).first().copied()
    }
}

pub(super) struct NoData;
impl Payload for NoData {
    type Data<'a> = NoData;
    const MAX_CHILDREN: usize = 0;
    fn get(_: &Elements, _: Key) -> NoData {
        NoData
    }
}

pub(super) struct Utf8;
impl Payload for Utf8 {
    type Data<'a> = &'a str;
    const MAX_CHILDREN: usize = 0;
    const USES_GLYPHS: bool = true;
    fn get(e: &Elements, k: Key) -> &str {
        e.text(k)
    }
    fn decode(tag: u8, r: &mut wire::Reader<'_>, raw: &[u8], a: &mut Attrs) -> Result<(), String> {
        if tag != 21 {
            return Err(format!("unsupported attribute {tag} in first slice"));
        }
        let s = r.string()?;
        if s.contains(['\r', '\n']) {
            return Err("multiline text is not in the first slice".into());
        }
        a.text = wire::range(raw, s.as_bytes());
        a.glyph_count = s.chars().count();
        Ok(())
    }
}
