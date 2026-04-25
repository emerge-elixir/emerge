#![allow(dead_code)]

use emerge_skia::tree::attrs::{
    Attrs, Background, BorderRadius, BorderWidth, Color, Length, Padding,
};
use emerge_skia::tree::element::{Element, ElementKind, ElementTree, NodeId};
use emerge_skia::tree::layout::TextMeasurer;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const TEXT_ROW_COUNT: usize = 500;
pub const CARD_COUNT: usize = 160;

pub struct FixtureScenario {
    pub id: String,
    pub full_emrg: Vec<u8>,
    patches: BTreeMap<String, Vec<u8>>,
}

impl FixtureScenario {
    pub fn patch_bytes(&self, mutation: &str) -> &[u8] {
        self.patches
            .get(mutation)
            .unwrap_or_else(|| panic!("unknown benchmark mutation: {mutation}"))
    }

    pub fn patch_names(&self) -> impl Iterator<Item = &str> {
        self.patches.keys().map(String::as_str)
    }
}

pub fn load_fixture(id: &str) -> FixtureScenario {
    let root = fixture_dir(id);
    FixtureScenario {
        id: id.to_string(),
        full_emrg: read_fixture(&root, "full.emrg"),
        patches: read_patch_fixtures(&root),
    }
}

pub fn load_fixtures() -> Vec<FixtureScenario> {
    let root = fixture_root();
    let mut ids: Vec<_> = std::fs::read_dir(&root)
        .unwrap_or_else(|err| {
            panic!(
                "failed to read benchmark fixture root {}: {err}. Run `mix bench.fixtures` first.",
                root.display()
            )
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();

    ids.sort();

    let fixtures: Vec<_> = ids
        .into_iter()
        .filter(|id| fixture_dir(id).join("full.emrg").is_file())
        .map(|id| load_fixture(&id))
        .collect();

    if fixtures.is_empty() {
        panic!(
            "benchmark fixture root {} does not contain generated fixtures. Run `mix bench.fixtures` first.",
            root.display()
        );
    }

    fixtures
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("bench")
        .join("fixtures")
}

fn fixture_dir(id: &str) -> PathBuf {
    fixture_root().join(id)
}

fn read_patch_fixtures(root: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    let patches: BTreeMap<String, Vec<u8>> = std::fs::read_dir(root)
        .unwrap_or_else(|err| panic!("failed to read benchmark fixture {}: {err}", root.display()))
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_name = entry.file_name().into_string().ok()?;
            let mutation = file_name.strip_suffix(".patch")?;
            Some((mutation.to_string(), read_fixture(root, &file_name)))
        })
        .collect();

    if patches.is_empty() {
        panic!(
            "benchmark fixture {} does not contain any patch files. Run `mix bench.fixtures` first.",
            root.display()
        );
    }

    patches
}

fn read_fixture(root: &std::path::Path, file_name: &str) -> Vec<u8> {
    let path = root.join(file_name);
    std::fs::read(&path).unwrap_or_else(|err| {
        panic!(
            "failed to read benchmark fixture {}: {err}. Run `mix bench.fixtures` first.",
            path.display()
        )
    })
}

pub struct MockTextMeasurer;

impl TextMeasurer for MockTextMeasurer {
    fn measure_with_font(
        &self,
        text: &str,
        font_size: f32,
        _family: &str,
        _weight: u16,
        _italic: bool,
    ) -> (f32, f32) {
        (text.chars().count() as f32 * font_size * 0.5, font_size)
    }

    fn font_metrics(
        &self,
        font_size: f32,
        _family: &str,
        _weight: u16,
        _italic: bool,
    ) -> (f32, f32) {
        (font_size * 0.75, font_size * 0.25)
    }
}

pub fn large_text_column(row_count: usize) -> ElementTree {
    let mut tree = ElementTree::new();
    let root_id = NodeId::from_u64(1);
    tree.set_root_id(root_id);

    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Fill);
    root_attrs.spacing = Some(2.0);
    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::Column,
        Vec::new(),
        root_attrs,
    ));

    let row_ids: Vec<_> = (0..row_count)
        .map(|index| {
            let row_id = NodeId::from_u64(10_000 + index as u64);
            let text_id = NodeId::from_u64(20_000 + index as u64);

            let mut row_attrs = Attrs::default();
            row_attrs.width = Some(Length::Fill);
            row_attrs.padding = Some(Padding::Uniform(2.0));

            let mut text_attrs = Attrs::default();
            text_attrs.content = Some(format!(
                "Benchmark row {index}: repeated text content for layout measurement"
            ));
            text_attrs.font_size = Some(16.0);

            tree.insert(Element::with_attrs(
                row_id,
                ElementKind::Row,
                Vec::new(),
                row_attrs,
            ));
            tree.insert(Element::with_attrs(
                text_id,
                ElementKind::Text,
                Vec::new(),
                text_attrs,
            ));
            tree.set_children(&row_id, vec![text_id])
                .expect("row child should exist");

            row_id
        })
        .collect();

    tree.set_children(&root_id, row_ids)
        .expect("root children should exist");
    tree
}

pub fn nested_card_grid(card_count: usize) -> ElementTree {
    let mut tree = ElementTree::new();
    let root_id = NodeId::from_u64(2);
    tree.set_root_id(root_id);

    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(960.0));
    root_attrs.spacing_x = Some(8.0);
    root_attrs.spacing_y = Some(8.0);
    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::WrappedRow,
        Vec::new(),
        root_attrs,
    ));

    let card_ids: Vec<_> = (0..card_count)
        .map(|index| {
            let base = 100_000 + index as u64 * 10;
            let card_id = NodeId::from_u64(base);
            let header_id = NodeId::from_u64(base + 1);
            let title_id = NodeId::from_u64(base + 2);
            let badge_id = NodeId::from_u64(base + 3);
            let body_id = NodeId::from_u64(base + 4);

            let mut card_attrs = Attrs::default();
            card_attrs.width = Some(Length::Px(280.0));
            card_attrs.padding = Some(Padding::Uniform(8.0));
            card_attrs.spacing = Some(6.0);
            card_attrs.background = Some(Background::Color(Color::Rgb {
                r: 245,
                g: 247,
                b: 250,
            }));
            card_attrs.border_radius = Some(BorderRadius::Uniform(8.0));
            card_attrs.border_width = Some(BorderWidth::Uniform(1.0));
            card_attrs.border_color = Some(Color::Rgb {
                r: 220,
                g: 226,
                b: 235,
            });

            let mut header_attrs = Attrs::default();
            header_attrs.width = Some(Length::Fill);
            header_attrs.spacing = Some(6.0);

            let mut title_attrs = Attrs::default();
            title_attrs.content = Some(format!("Card {index}"));
            title_attrs.font_size = Some(18.0);

            let mut badge_attrs = Attrs::default();
            badge_attrs.content = Some(format!("#{index}"));
            badge_attrs.font_size = Some(12.0);

            let mut body_attrs = Attrs::default();
            body_attrs.content = Some(
                "Nested benchmark body copy with enough words to exercise text measurement."
                    .to_string(),
            );
            body_attrs.font_size = Some(14.0);

            tree.insert(Element::with_attrs(
                card_id,
                ElementKind::Column,
                Vec::new(),
                card_attrs,
            ));
            tree.insert(Element::with_attrs(
                header_id,
                ElementKind::Row,
                Vec::new(),
                header_attrs,
            ));
            tree.insert(Element::with_attrs(
                title_id,
                ElementKind::Text,
                Vec::new(),
                title_attrs,
            ));
            tree.insert(Element::with_attrs(
                badge_id,
                ElementKind::Text,
                Vec::new(),
                badge_attrs,
            ));
            tree.insert(Element::with_attrs(
                body_id,
                ElementKind::Text,
                Vec::new(),
                body_attrs,
            ));

            tree.set_children(&header_id, vec![title_id, badge_id])
                .expect("header children should exist");
            tree.set_children(&card_id, vec![header_id, body_id])
                .expect("card children should exist");

            card_id
        })
        .collect();

    tree.set_children(&root_id, card_ids)
        .expect("root children should exist");
    tree
}

pub fn reversed_root_children(tree: &ElementTree) -> Vec<NodeId> {
    let root_id = tree.root_id().expect("tree should have a root");
    let mut children = tree.live_child_ids(&root_id);
    children.reverse();
    children
}

pub fn attr_patch_id(row_index: usize) -> NodeId {
    NodeId::from_u64(10_000 + row_index as u64)
}

pub fn paint_attr_raw() -> Vec<u8> {
    vec![0, 1, 12, 0, 1, 255, 0, 0, 255]
}
