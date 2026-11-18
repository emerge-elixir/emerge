use super::{Direction::*, *};
use slotmap::SlotMap;

#[test]
fn scans_across_empty_words_in_both_directions() {
    let depths = [64, MAX_DEPTH, 1, 191, 0, 63, 511, 65];
    let mut slots = SlotMap::<Key, _>::with_key();
    let keys = depths.map(|d| slots.insert(d));
    let mut set = WorkSet::default();
    set.reserve(slots.capacity(), MAX_DEPTH + 1).unwrap();
    for direction in [ShallowFirst, DeepFirst] {
        for &k in &keys {
            set.push(slots[k], k, Outputs::Draw)
        }
        let drained: Vec<_> = std::iter::from_fn(|| set.pop(direction))
            .map(|(k, outputs)| {
                assert_eq!(outputs, Outputs::Draw);
                slots[k]
            })
            .collect();
        let mut expected = depths;
        expected.sort_unstable();
        if matches!(direction, DeepFirst) {
            expected.reverse()
        }
        assert_eq!(drained, expected);
    }
}

#[test]
fn work_added_while_draining_updates_extremes_and_merges_per_mount() {
    let mut slots = SlotMap::<Key, _>::with_key();
    let [a, b, shallow, deep] = std::array::from_fn(|_| slots.insert(()));
    let mut set = WorkSet::default();
    set.reserve(slots.capacity(), MAX_DEPTH + 1).unwrap();
    set.push(64, a, Outputs::Draw);
    set.push(64, b, Outputs::Hit);
    for side in [Outputs::Hit, Outputs::Draw] {
        set.push(64, a, side)
    }

    let first = set.pop(ShallowFirst).unwrap();
    set.push(0, shallow, Outputs::Draw);
    set.push(MAX_DEPTH, deep, Outputs::Hit);
    assert_eq!(set.pop(DeepFirst), Some((deep, Outputs::Hit)));
    assert_eq!(set.pop(ShallowFirst), Some((shallow, Outputs::Draw)));
    let second = set.pop(DeepFirst).unwrap();
    assert!(first.0 != second.0);
    assert!([first, second].contains(&(a, Outputs::Both)));
    assert!([first, second].contains(&(b, Outputs::Hit)));
    assert!(set.pop(ShallowFirst).is_none());

    set.push(64, a, Outputs::Draw);
    assert_eq!(set.pop(ShallowFirst), Some((a, Outputs::Draw)));
}

#[test]
fn discarding_pending_work_keeps_storage_and_outputs_separate_across_mounts() {
    let mut slots = SlotMap::<Key, _>::with_key();
    let a = slots.insert(());
    let old = slots.insert(());
    let mut set = WorkSet::default();
    set.reserve(slots.capacity(), MAX_DEPTH + 1).unwrap();
    set.push(1, a, Outputs::Draw);
    set.push(MAX_DEPTH, old, Outputs::Hit);
    set.clear();
    assert!(set.pop(DeepFirst).is_none());
    slots.remove(old).unwrap();
    let new = slots.insert(());
    assert_ne!(old, new);
    set.push(0, new, Outputs::Draw);
    set.push(MAX_DEPTH, old, Outputs::Hit); // stale generation cannot extend a newer pending entry
    assert_eq!(set.pop(ShallowFirst), Some((new, Outputs::Draw)));
    assert!(set.pop(ShallowFirst).is_none());
}
