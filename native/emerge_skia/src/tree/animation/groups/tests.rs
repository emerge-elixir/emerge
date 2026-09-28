use super::*;
use crate::tree::element::{Element, ElementKind, NearbyMount};
use std::time::Duration;

fn id(n: u64) -> NodeId {
    NodeId::from_wire_u64(n)
}
fn group(n: u64) -> GroupKey {
    GroupKey::Flow(Mount {
        id: id(n),
        revision: 1,
    })
}
fn input(
    node: u64,
    parent: u64,
    start: Instant,
    duration: u64,
    now: Instant,
    resolved: bool,
) -> MemberInput {
    let deadline = start + Duration::from_millis(duration);
    MemberInput {
        owner: OwnerKey {
            node: id(node),
            mount: 1,
            kind: Owner::Regular,
            generation: RunGeneration(node),
            started: start,
        },
        group: group(parent),
        deadline,
        active: now < deadline,
        resolved,
        segment: SegmentStamp {
            index: 0,
            start_ms_bits: 0.0_f64.to_bits(),
        },
    }
}
#[test]
fn early_finisher_is_retained_only_until_its_parent_barrier() {
    let start = Instant::now();
    let mut ledger = GroupLedger::default();
    let inputs = |now| {
        [
            input(2, 1, start, 1000, now, true),
            input(3, 1, start, 2000, now, true),
        ]
    };
    ledger.refresh(inputs(start), start);
    assert_eq!(ledger.len(), 1);
    let half = start + Duration::from_millis(1000);
    ledger.refresh(inputs(half), half);
    assert_eq!(ledger.group(inputs(half)[0].owner), Some(group(1)));
    assert_eq!(
        ledger.barrier(group(1)),
        Some(start + Duration::from_millis(2000))
    );
    let end = start + Duration::from_millis(2000);
    ledger.refresh(inputs(end), end);
    assert_eq!(
        ledger.len(),
        1,
        "ready owners survive until successful preparation"
    );
    ledger.commit(&ledger.ready(end));
    assert_eq!(ledger.len(), 0);
    assert!(ledger.owners.is_empty());
    ledger.refresh(inputs(end), end);
    assert!(
        ledger.owners.is_empty(),
        "completed declarations cannot create idle groups"
    );
}
#[test]
fn arrivals_extend_only_their_parent_and_cancellation_releases_holds() {
    let start = Instant::now();
    let mut ledger = GroupLedger::default();
    let a = input(2, 1, start, 1000, start, true);
    let b = input(3, 1, start, 2000, start, true);
    let foreign = input(5, 4, start, 5000, start, true);
    ledger.refresh([a, b, foreign], start);
    let now = start + Duration::from_millis(1500);
    let a = MemberInput { active: false, ..a };
    let arrival = input(6, 1, now, 2000, now, true);
    ledger.refresh([a, b, foreign, arrival], now);
    assert_eq!(
        ledger.barrier(group(1)),
        Some(start + Duration::from_millis(3500))
    );
    ledger.refresh([a, foreign], now);
    assert_eq!(ledger.ready(now), HashSet::from([group(1)]));
    ledger.commit(&ledger.ready(now));
    assert!(ledger.group(a.owner).is_none());
    assert_eq!(ledger.len(), 1);
    assert_eq!(
        ledger.barrier(group(4)),
        Some(start + Duration::from_millis(5000))
    );
}
#[test]
fn ownership_boundaries_follow_parent_mount_and_nearby_slot() {
    let mut tree = ElementTree::new();
    tree.set_root_id(id(1));
    for n in 1..=5 {
        tree.insert(Element::with_attrs(
            id(n),
            ElementKind::Row,
            vec![],
            Attrs::default(),
        ));
    }
    tree.set_children(&id(1), vec![id(2), id(3)]).unwrap();
    tree.set_children(&id(2), vec![id(4)]).unwrap();
    tree.set_nearby_mounts(
        &id(2),
        vec![NearbyMount {
            slot: NearbySlot::Above,
            id: id(5),
        }],
    )
    .unwrap();
    assert_eq!(
        GroupKey::for_node(&tree, id(2)),
        GroupKey::for_node(&tree, id(3))
    );
    assert_ne!(
        GroupKey::for_node(&tree, id(2)),
        GroupKey::for_node(&tree, id(4))
    );
    assert_ne!(
        GroupKey::for_node(&tree, id(4)),
        GroupKey::for_node(&tree, id(5))
    );
    let before = GroupKey::for_node(&tree, id(4));
    tree.get_mut(&id(2)).unwrap().lifecycle.mounted_at_revision += 1;
    assert_ne!(GroupKey::for_node(&tree, id(4)), before);
    tree.set_nearby_mounts(
        &id(2),
        vec![NearbyMount {
            slot: NearbySlot::Below,
            id: id(5),
        }],
    )
    .unwrap();
    assert!(matches!(
        GroupKey::for_node(&tree, id(5)),
        Some(GroupKey::Nearby(_, NearbySlot::Below))
    ));
}
#[test]
fn paint_fields_and_unbounded_clocks_do_not_form_finite_layout_barriers() {
    assert!(!has_layout_fields(&Attrs {
        alpha: Some(0.5),
        move_x: Some(40.0),
        rotate: Some(45.0),
        ..Attrs::default()
    }));
    assert!(has_layout_fields(&Attrs {
        layout_rotate: Some(45.0),
        ..Attrs::default()
    }));
    let start = Instant::now();
    let mut spec = AnimationSpec {
        keyframes: vec![Attrs::default(); 2],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    };
    assert_eq!(timing::finite_deadline(&spec, start), None);
    spec.repeat = AnimationRepeat::Times(3);
    assert_eq!(
        timing::finite_deadline(&spec, start),
        Some(start + Duration::from_millis(300))
    );
    spec.duration_ms = f64::INFINITY;
    assert_eq!(timing::finite_deadline(&spec, start), None);
    spec.duration_ms = f64::NAN;
    assert_eq!(timing::finite_deadline(&spec, start), None);
    let mut ledger = GroupLedger::default();
    ledger.refresh([input(2, 1, start, 1000, start, false)], start);
    assert_eq!(
        ledger.len(),
        0,
        "compatible-only runs retain their existing fast path"
    );
}

#[test]
fn group_tickets_reuse_warm_versions_but_reject_membership_phase_and_clock_changes() {
    let start = Instant::now();
    let a = input(2, 1, start, 2000, start, true);
    let mut ledger = GroupLedger::default();
    ledger.refresh([a], start);
    let ticket = ledger.ticket(&HashSet::from([a.group])).unwrap();
    ledger.refresh([a], start + Duration::from_millis(500));
    assert!(
        ticket.valid_for(&ledger),
        "progress alone is not a new target epoch"
    );
    for changed in [
        MemberInput {
            owner: OwnerKey {
                generation: RunGeneration(90),
                ..a.owner
            },
            ..a
        },
        MemberInput {
            segment: SegmentStamp {
                index: 1,
                start_ms_bits: 1000.0_f64.to_bits(),
            },
            ..a
        },
        MemberInput {
            segment: SegmentStamp {
                index: 0,
                start_ms_bits: 2000.0_f64.to_bits(),
            },
            ..a
        },
        MemberInput {
            deadline: a.deadline + Duration::from_millis(1),
            ..a
        },
        MemberInput {
            owner: OwnerKey {
                started: start + Duration::from_millis(1),
                ..a.owner
            },
            ..a
        },
    ] {
        let mut altered = ledger.clone();
        altered.refresh([changed], start);
        assert!(!ticket.valid_for(&altered));
    }
    ledger.refresh([a, input(4, 3, start, 3000, start, true)], start);
    assert!(
        ticket.valid_for(&ledger),
        "foreign membership does not version this parent"
    );
    ledger.refresh([a, input(5, 1, start, 3000, start, true)], start);
    assert!(!ticket.valid_for(&ledger));
}

#[test]
fn a_ticket_cannot_be_replayed_after_release_or_parent_recreation() {
    let start = Instant::now();
    let a = input(2, 1, start, 1000, start, true);
    let mut ledger = GroupLedger::default();
    ledger.refresh([a], start);
    let keys = HashSet::from([a.group]);
    let ticket = ledger.ticket(&keys).unwrap();
    ledger.commit(&keys);
    assert!(!ticket.valid_for(&ledger));
    ledger.refresh([a], start);
    assert!(
        !ticket.valid_for(&ledger),
        "even identical reconstructed membership is a new version"
    );
}

#[test]
fn warm_membership_does_not_copy_groups_and_phase_edits_copy_only_the_changed_parent() {
    let start = Instant::now();
    let mut ledger = GroupLedger::default();
    let members = [
        input(2, 1, start, 2000, start, true),
        input(3, 1, start, 2000, start, true),
        input(5, 4, start, 2000, start, true),
    ];
    ledger.refresh(members, start);
    ledger.commit_admissions();
    let first = Arc::clone(ledger.groups.get(&group(1)).unwrap());
    let foreign = Arc::clone(ledger.groups.get(&group(4)).unwrap());
    let copies = ledger.work;
    for ms in [10, 30, 100, 250, 500] {
        ledger.refresh(members, start + Duration::from_millis(ms));
        assert!(!ledger.has_staged());
    }
    assert_eq!(ledger.work.group_copies, copies.group_copies);
    assert_eq!(ledger.work.member_copies, copies.member_copies);
    assert_eq!(
        ledger.work.input_visits - copies.input_visits,
        5 * 5 * members.len() as u64
    );
    let mut next = members;
    next[0].segment.index = 1;
    ledger.refresh(next, start + Duration::from_millis(600));
    assert_eq!(ledger.work.group_copies - copies.group_copies, 1);
    assert_eq!(ledger.work.member_copies - copies.member_copies, 2);
    assert!(!Arc::ptr_eq(&first, ledger.groups.get(&group(1)).unwrap()));
    assert!(Arc::ptr_eq(&foreign, ledger.groups.get(&group(4)).unwrap()));
    ledger.refresh(members, start + Duration::from_millis(600));
    assert!(Arc::ptr_eq(&first, ledger.groups.get(&group(1)).unwrap()));
    assert!(!ledger.has_staged());
}

#[test]
fn all_selected_owner_kinds_have_the_same_disjoint_geometry_phases() {
    use crate::tree::animation::{change::Field, fields::FieldMask};
    for kind in [
        Owner::Regular,
        Owner::Enter,
        Owner::Exit,
        Owner::Change(Field::Width),
    ] {
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();
        let mut first = input(1, 100, start, 1000, start, true);
        first.owner.kind = kind;
        let second = input(2, 100, start, 2000, start, true);
        runtime.groups.refresh(vec![first, second], start);
        let width = FieldMask::field(Field::Width);
        let alpha = FieldMask::field(Field::Alpha);
        let fields = width.union(alpha);
        let phase = |active, ms| {
            runtime.owner_phases(
                first.owner,
                fields,
                active,
                Some(start + Duration::from_millis(ms)),
                FieldMask::default(),
            )
        };
        assert_eq!(phase(true, 500).moving, fields);
        let held = phase(false, 1000);
        assert_eq!(held.held, width);
        assert!(held.moving.is_empty());
        assert!(held.pending.is_empty());
        let releasing = phase(false, 2000);
        assert_eq!(releasing.releasing, width);
        assert!(releasing.held.is_empty());
    }
}
