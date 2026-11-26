//! An axis-mixing layout turn is one item on the surrounding line. Its
//! words are still ordinary prose fragments in their original local axes.
use super::*;

/// When the Text itself takes space on turned axes, the unturned wrapper
/// surrounds that footprint. Its padding and rounded decoration belong to
/// the wrapper, not to the Text's coordinate system.
pub(super) fn owner_box(e: &Elements, owner: Key, source: Key, ink: Rect) -> Rect {
    let inner = e.attrs[source].rotation;
    if source == owner || !inner.mixes_axes() {
        return ink;
    }
    let r = inner.about_center(ink);
    let [top, right, bottom, left] = insets(e, owner, source);
    Rect {
        x: coordinate(r.x - left),
        y: coordinate(r.y - top),
        width: coordinate(r.width + (left + right)),
        height: coordinate(r.height + (top + bottom)),
    }
}

pub(super) fn mixes_axes(e: &Elements, owner: Key, source: Key) -> bool {
    e.attrs[owner].rotation.mixes_axes() || e.attrs[source].rotation.mixes_axes()
}

pub(super) fn slot(e: &Elements, owner: Key, source: Key, ink: Rect) -> Rect {
    e.attrs[owner]
        .rotation
        .about_center(owner_box(e, owner, source, ink))
}

impl Lines<'_> {
    pub(super) fn turned_span(&mut self, phrase: Phrase<'_>, text: &TextStore) {
        let e = self.elements;
        let first = self.words.len();
        let Phrase { owner, source, .. } = phrase;
        let nested = owner != source && e.attrs[source].rotation.mixes_axes();
        let pad = if nested { [0.; 4] } else { phrase.pad };
        // Shape the phrase once, straight into this paragraph's admitted
        // fragment storage. The parent's remaining width chooses *where* this
        // item fits, never an artificial narrow width for its individual words.
        let mut clear = Floats::default();
        let mut local = Lines::new(
            e,
            self.words,
            &mut clear,
            Band::new([0.; 2], 0., f32::MAX, 0.),
            0.,
            0.,
            Alignment::Start,
        );
        local.span(Phrase { pad, ..phrase }, text);
        let pen = local.x;
        let Some(head) = self.words.get(first) else {
            return;
        };
        let ink = union(head.bounds, self.words.last().unwrap().bounds);
        let slot = slot(e, owner, source, ink);
        let right = self.room(first, slot.height, true, |_| slot.width);
        let (dx, dy) = (self.x - slot.x, self.y - slot.y);
        for word in &mut self.words[first..] {
            word.shift(dx, dy);
        }
        self.x = coordinate(self.x + slot.width);
        self.occupy(
            e.attrs[owner].align[1].unwrap_or_default(),
            slot.height,
            self.x,
            right,
        );
        // Direct Text's trailing separator advances the cursor but is not a
        // decorated/clickable part of its last word.
        if pen > ink.x + ink.width {
            self.space(pen - (ink.x + ink.width), 0., true, 0.);
        }
    }
}
