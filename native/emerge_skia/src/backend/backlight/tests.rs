use super::*;
use std::{
    collections::{HashMap, VecDeque},
    fs,
    os::unix::fs::symlink,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
struct FakeState {
    values: HashMap<Attribute, u32>,
    writes: VecDeque<(Attribute, u32)>,
    failures: VecDeque<(Attribute, u32, bool)>,
    read_failure: Option<Attribute>,
}
#[derive(Clone)]
struct Fake(Arc<Mutex<FakeState>>);
impl Fake {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(FakeState {
            values: [
                (Attribute::Maximum, 100),
                (Attribute::Brightness, 37),
                (Attribute::Power, 0),
                (Attribute::Actual, 5),
            ]
            .into_iter()
            .collect(),
            ..FakeState::default()
        })))
    }
    fn writes(&self) -> Vec<(Attribute, u32)> {
        self.0.lock().unwrap().writes.iter().copied().collect()
    }
    fn fail(&self, attribute: Attribute, value: u32, after_effect: bool) {
        self.0
            .lock()
            .unwrap()
            .failures
            .push_back((attribute, value, after_effect));
    }
}
impl Attributes for Fake {
    fn read(&mut self, attribute: Attribute) -> Result<Option<u32>, String> {
        let state = self.0.lock().unwrap();
        if state.read_failure == Some(attribute) {
            return Err("permission denied".into());
        }
        Ok(state.values.get(&attribute).copied())
    }
    fn write(&mut self, attribute: Attribute, value: u32) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        state.writes.push_back((attribute, value));
        let failure = state
            .failures
            .front()
            .copied()
            .filter(|(a, v, _)| *a == attribute && *v == value);
        if failure.is_some() {
            state.failures.pop_front();
        }
        if failure.is_some_and(|(_, _, after)| !after) {
            return Err("injected write failure".into());
        }
        state.values.insert(attribute, value);
        if failure.is_some() {
            return Err("injected failure after effect".into());
        }
        Ok(())
    }
}
fn id(inode: u64) -> DeviceId {
    DeviceId { device: 1, inode }
}
fn managed(fake: &Fake, registry: &Reservations) -> Backlight<Fake> {
    Backlight::prepare(fake.clone(), registry.reserve(id(1)).unwrap(), None).unwrap()
}

#[test]
fn quantization_is_finite_bounded_and_rounds_half_up() {
    assert_eq!(quantize(0.0, 7).unwrap(), 0);
    assert_eq!(quantize(0.5, 7).unwrap(), 4);
    assert_eq!(quantize(1.0, u32::MAX).unwrap(), u32::MAX);
    assert_eq!(quantize(0.5, u32::MAX).unwrap(), 2_147_483_648);
    assert!(quantize(0.5, 0).is_err());
    for value in [-0.1, 1.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(quantize(value, 100).is_err());
    }
}

#[test]
fn preparation_is_read_only_and_uses_requested_not_actual_brightness() {
    let fake = Fake::new();
    let registry = Reservations::new();
    let mut light = managed(&fake, &registry);
    assert_eq!(light.baseline().requested, 37);
    assert_eq!(light.actual_level().unwrap(), Some(5));
    assert!(light.illuminate_after_present().is_err());
    light.set_brightness(Some(0.7)).unwrap();
    assert!(fake.writes().is_empty());
    assert_eq!(light.desired_level(), 70);
    drop(light); // Untouched binding can be abandoned without writes or retention.
    assert!(registry.reserve(id(1)).is_ok());
}

#[test]
fn dark_updates_preserve_intent_and_enable_sets_brightness_before_power() {
    use Attribute::*;
    let fake = Fake::new();
    let registry = Reservations::new();
    let mut light = managed(&fake, &registry);
    light.darken().unwrap();
    light.set_brightness(Some(0.42)).unwrap();
    assert_eq!(fake.writes(), [(Power, 4), (Brightness, 0)]);
    light.illuminate_after_present().unwrap();
    assert_eq!(
        fake.writes(),
        [(Power, 4), (Brightness, 0), (Brightness, 42), (Power, 0)]
    );
    light.release_after_detach().unwrap();
    assert!(registry.reserve(id(1)).is_ok());
    assert_eq!(fake.0.lock().unwrap().values[&Power], 4);
    assert_eq!(fake.0.lock().unwrap().values[&Brightness], 0);
}

#[test]
fn visible_zero_is_not_power_off_and_nil_uses_the_original_baseline() {
    use Attribute::*;
    let fake = Fake::new();
    let registry = Reservations::new();
    let mut light = managed(&fake, &registry);
    light.darken().unwrap();
    light.illuminate_after_present().unwrap();
    let count = fake.writes().len();
    light.set_brightness(Some(0.0)).unwrap();
    assert_eq!(&fake.writes()[count..], [(Brightness, 0)]);
    light.darken().unwrap();
    light.illuminate_after_present().unwrap();
    assert_eq!(light.desired_level(), 0);
    assert_eq!(fake.0.lock().unwrap().values[&Power], 0);
    light.set_brightness(None).unwrap();
    assert_eq!(light.desired_level(), 37);
    assert_eq!(fake.0.lock().unwrap().values[&Brightness], 37);
    light.release_after_detach().unwrap();
}

#[test]
fn suppression_attempts_both_channels_and_never_treats_partial_success_as_dark() {
    use Attribute::*;
    let fake = Fake::new();
    fake.fail(Power, 4, false);
    let registry = Reservations::new();
    let mut light = managed(&fake, &registry);
    assert!(light.darken().is_err());
    assert_eq!(fake.writes(), [(Power, 4), (Brightness, 0)]);
    assert!(light.illuminate_after_present().is_err());
    let count = fake.writes().len();
    assert!(light.set_brightness(Some(0.8)).is_err());
    assert_eq!(light.desired_level(), 80);
    assert_eq!(fake.writes().len(), count); // Unknown state never brightens.
    drop(light);
    assert!(registry.reserve(id(1)).is_err());
    assert!(registry.reserve(id(2)).is_ok());
}

#[test]
fn brightness_failure_never_proceeds_to_unblanking() {
    use Attribute::*;
    let fake = Fake::new();
    let registry = Reservations::new();
    let mut light = managed(&fake, &registry);
    light.darken().unwrap();
    fake.fail(Brightness, 37, false);
    assert!(light.illuminate_after_present().is_err());
    assert!(!fake.writes().contains(&(Power, 0)));
    assert_eq!(fake.0.lock().unwrap().values[&Brightness], 0);
    light.release_after_detach().unwrap();
}

#[test]
fn failure_after_unblank_effect_resuppresses_and_preserves_requested_level() {
    use Attribute::*;
    let fake = Fake::new();
    let registry = Reservations::new();
    let mut light = managed(&fake, &registry);
    light.darken().unwrap();
    fake.fail(Power, 0, true);
    assert!(light.illuminate_after_present().is_err());
    assert_eq!(fake.0.lock().unwrap().values[&Power], 4);
    assert_eq!(fake.0.lock().unwrap().values[&Brightness], 0);
    assert_eq!(light.desired_level(), 37);
    light.release_after_detach().unwrap();
}

#[test]
fn failed_cleanup_and_surrounding_quarantine_keep_the_identity_reserved() {
    use Attribute::*;
    let registry = Reservations::new();
    let fake = Fake::new();
    let light = managed(&fake, &registry);
    fake.fail(Power, 4, false);
    fake.fail(Brightness, 0, false);
    let error = light.release_after_detach().unwrap_err();
    assert!(error.contains("power-down failed"));
    assert!(error.contains("zero brightness failed"));
    assert!(registry.reserve(id(1)).is_err());

    let registry = Reservations::new();
    let mut light = managed(&Fake::new(), &registry);
    light.retain();
    light.release_after_detach().unwrap();
    assert!(registry.reserve(id(1)).is_err());
}

#[test]
fn drop_never_restores_or_performs_io_and_unclean_exit_retains_admission() {
    let fake = Fake::new();
    let registry = Reservations::new();
    let mut light = managed(&fake, &registry);
    light.darken().unwrap();
    light.illuminate_after_present().unwrap();
    let writes = fake.writes();
    drop(light);
    assert_eq!(fake.writes(), writes);
    assert!(registry.reserve(id(1)).is_err());
}

#[test]
fn recovery_reuses_the_baseline_instead_of_recapturing_our_zero() {
    let fake = Fake::new();
    let registry = Reservations::new();
    let light = managed(&fake, &registry);
    let baseline = light.baseline();
    light.release_after_detach().unwrap();
    let light = Backlight::prepare(
        fake.clone(),
        registry.reserve(id(1)).unwrap(),
        Some(baseline),
    )
    .unwrap();
    assert_eq!(light.desired_level(), 37);
    assert_eq!(light.baseline(), baseline);
    drop(light);
    assert!(
        Backlight::prepare(
            fake.clone(),
            registry.reserve(id(2)).unwrap(),
            Some(baseline)
        )
        .is_err()
    );
    assert!(registry.reserve(id(2)).is_ok());
}

#[test]
fn invalid_values_and_read_failures_do_not_write_or_leak_admission() {
    let registry = Reservations::new();
    let fake = Fake::new();
    fake.0.lock().unwrap().read_failure = Some(Attribute::Maximum);
    assert!(Backlight::prepare(fake.clone(), registry.reserve(id(1)).unwrap(), None).is_err());
    assert!(fake.writes().is_empty());
    fake.0.lock().unwrap().read_failure = None;
    fake.0.lock().unwrap().values.insert(Attribute::Maximum, 0);
    assert!(Backlight::prepare(fake.clone(), registry.reserve(id(1)).unwrap(), None).is_err());
    fake.0
        .lock()
        .unwrap()
        .values
        .insert(Attribute::Maximum, 100);
    let mut light = managed(&fake, &registry);
    assert!(light.set_brightness(Some(f64::NAN)).is_err());
    assert_eq!(light.desired_level(), 37);
    assert!(fake.writes().is_empty());
}

#[test]
fn concurrent_reservation_has_one_winner_and_aliases_share_the_same_identity() {
    let registry = Reservations::new();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let workers = (0..8)
        .map(|_| {
            let registry = registry.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let result = registry.reserve(id(9)).ok();
                barrier.wait(); // Keep any winner alive until all attempts finish.
                result
            })
        })
        .collect::<Vec<_>>();
    let winners = workers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(winners.len(), 1);
    assert!(registry.reserve(id(9)).is_err());
    drop(winners);
    assert!(registry.reserve(id(9)).is_ok());
}

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "emerge-backlight-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(name);
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("class")).unwrap();
        Self(root)
    }
    fn directory(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn device(&self, name: &str, connector: Option<&Path>) -> PathBuf {
        let path = self.directory(&format!("devices/{name}"));
        for (name, value) in [
            ("max_brightness", "9\n"),
            ("brightness", "1\n"),
            ("bl_power", "0\n"),
        ] {
            fs::write(path.join(name), value).unwrap();
        }
        if let Some(connector) = connector {
            symlink(connector, path.join("device")).unwrap();
        }
        symlink(&path, self.0.join("class").join(name)).unwrap();
        path
    }
    fn candidates(&self, connectors: &[PathBuf]) -> Vec<Candidate> {
        discover(&self.0.join("class"), connectors).unwrap()
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn discovery_requires_proven_association_and_deduplicates_aliases() {
    let tree = Tree::new();
    let gpu = tree.directory("gpu");
    let connector = tree.directory("gpu/drm/card0/card0-eDP-1");
    let other = tree.directory("gpu/drm/card0/card0-eDP-10");
    let panel = tree.device("panel", Some(&connector));
    let unknown = tree.device("generic", Some(&gpu));
    let wrong = tree.device("other", Some(&other));
    symlink(&panel, tree.0.join("class/alias")).unwrap();
    let candidates = tree.candidates(&[connector.clone(), connector.clone(), other]);
    assert_eq!(candidates.len(), 3);
    let selected = select(&candidates, &connector, &Selection::Auto)
        .unwrap()
        .unwrap();
    assert_eq!(selected.path, panel);
    assert!(select(&candidates, &connector, &Selection::Explicit(wrong)).is_err());
    assert!(
        select(&candidates, &connector, &Selection::Explicit(unknown))
            .unwrap()
            .is_some()
    );
    let alias = select(
        &candidates,
        &connector,
        &Selection::Explicit(tree.0.join("class/alias")),
    )
    .unwrap()
    .unwrap();
    assert_eq!(alias.identity, selected.identity);
    let registry = Reservations::new();
    let _guard = registry.reserve(selected.identity).unwrap();
    assert!(registry.reserve(alias.identity).is_err());
}

#[test]
fn missing_unknown_and_ambiguous_devices_are_not_guessed() {
    let tree = Tree::new();
    let connector = tree.directory("connector");
    assert!(discover(&tree.0.join("absent"), &[]).unwrap().is_empty());
    assert!(
        select(&[], &tree.0.join("absent"), &Selection::Unmanaged)
            .unwrap()
            .is_none()
    );
    tree.device("unknown", None);
    let candidates = tree.candidates(std::slice::from_ref(&connector));
    assert!(
        select(&candidates, &connector, &Selection::Auto)
            .unwrap()
            .is_none()
    );
    assert!(
        select(
            &candidates,
            &connector,
            &Selection::Explicit(tree.0.clone())
        )
        .is_err()
    );
    tree.device("panel1", Some(&connector));
    tree.device("panel2", Some(&connector));
    let candidates = tree.candidates(std::slice::from_ref(&connector));
    assert!(select(&candidates, &connector, &Selection::Auto).is_err());
}

#[test]
fn pinned_descriptors_never_follow_a_replacement_device_path() {
    let tree = Tree::new();
    let path = tree.device("panel", None);
    let candidates = tree.candidates(&[]);
    let mut io = SysfsAttributes::open(&candidates[0]).unwrap();
    let old = tree.0.join("old");
    fs::rename(&path, &old).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("brightness"), "2\n").unwrap();
    io.write(Attribute::Brightness, 0).unwrap();
    assert_eq!(fs::read_to_string(old.join("brightness")).unwrap(), "0\n");
    assert_eq!(fs::read_to_string(path.join("brightness")).unwrap(), "2\n");
    assert_eq!(io.read(Attribute::Actual).unwrap(), None);
    // Even opening attributes after discovery uses the pinned original directory.
    assert_eq!(
        SysfsAttributes::open(&candidates[0])
            .unwrap()
            .read(Attribute::Brightness)
            .unwrap(),
        Some(0)
    );
}

#[test]
fn symlink_attributes_and_invalid_sysfs_values_fail_without_writes() {
    let tree = Tree::new();
    let path = tree.device("panel", None);
    let candidates = tree.candidates(&[]);
    fs::remove_file(path.join("brightness")).unwrap();
    let outside = tree.0.join("outside");
    fs::write(&outside, "7\n").unwrap();
    symlink(&outside, path.join("brightness")).unwrap();
    let registry = Reservations::new();
    assert!(Backlight::open(&candidates[0], &registry, None).is_err());
    assert!(registry.reserve(candidates[0].identity).is_ok());
    assert_eq!(fs::read_to_string(&outside).unwrap(), "7\n");
    fs::remove_file(path.join("brightness")).unwrap();
    fs::write(path.join("brightness"), "-1\n").unwrap();
    assert!(Backlight::open(&candidates[0], &registry, None).is_err());
    fs::write(path.join("brightness"), "1\n").unwrap();
    fs::write(path.join("max_brightness"), "1".repeat(100)).unwrap();
    assert!(Backlight::open(&candidates[0], &registry, None).is_err());
    assert_eq!(fs::read_to_string(path.join("bl_power")).unwrap(), "0\n");
}

#[test]
fn connector_ancestry_without_a_device_link_is_also_positive_evidence() {
    let tree = Tree::new();
    let connector = tree.directory("connector");
    let original = tree.device("panel", None);
    let parent = connector.join("backlight");
    fs::create_dir(&parent).unwrap();
    let nested = parent.join("panel");
    fs::rename(original, &nested).unwrap();
    fs::remove_file(tree.0.join("class/panel")).unwrap();
    symlink(&nested, tree.0.join("class/panel")).unwrap();
    let candidates = tree.candidates(std::slice::from_ref(&connector));
    assert_eq!(
        select(&candidates, &connector, &Selection::Auto)
            .unwrap()
            .unwrap()
            .path,
        nested
    );
}

#[test]
fn incomplete_inventory_and_relative_explicit_paths_fail_closed() {
    let tree = Tree::new();
    let connector = tree.directory("connector");
    let candidates = tree.candidates(std::slice::from_ref(&connector));
    assert!(
        select(
            &candidates,
            &connector,
            &Selection::Explicit("panel".into())
        )
        .is_err()
    );
    symlink(tree.0.join("absent"), tree.0.join("class/broken")).unwrap();
    assert!(discover(&tree.0.join("class"), &[connector]).is_err());
}

#[test]
fn visible_write_failure_cannot_be_erased_by_a_later_intent_only_update() {
    use Attribute::*;
    let fake = Fake::new();
    let registry = Reservations::new();
    let mut light = managed(&fake, &registry);
    light.darken().unwrap();
    light.illuminate_after_present().unwrap();
    fake.fail(Brightness, 60, true);
    assert!(light.set_brightness(Some(0.6)).is_err());
    let writes = fake.writes();
    assert!(light.set_brightness(Some(0.7)).is_err());
    assert_eq!(light.desired_level(), 70);
    assert_eq!(fake.writes(), writes);
    light.darken().unwrap();
    light.illuminate_after_present().unwrap();
    assert_eq!(fake.0.lock().unwrap().values[&Brightness], 70);
    light.release_after_detach().unwrap();
}

#[test]
fn enable_and_resuppression_failures_are_both_reported_and_retain_ownership() {
    use Attribute::*;
    let fake = Fake::new();
    let registry = Reservations::new();
    let mut light = managed(&fake, &registry);
    light.darken().unwrap();
    fake.fail(Power, 0, true);
    fake.fail(Power, 4, false);
    fake.fail(Brightness, 0, false);
    let error = light.illuminate_after_present().unwrap_err();
    assert!(error.contains("failure after effect"));
    assert!(error.contains("suppression after failed enable also failed"));
    assert!(light.illuminate_after_present().is_err());
    drop(light);
    assert!(registry.reserve(id(1)).is_err());
}

#[test]
fn production_registry_clones_share_admission() {
    let identity = DeviceId {
        device: u64::MAX,
        inode: u64::MAX,
    };
    let guard = Reservations::process_wide().reserve(identity).unwrap();
    assert!(Reservations::process_wide().reserve(identity).is_err());
    drop(guard);
    assert!(Reservations::process_wide().reserve(identity).is_ok());
}
