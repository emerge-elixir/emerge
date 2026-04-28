#![allow(dead_code)]

use emerge_skia::tree::animation::{AnimationCurve, AnimationRepeat, AnimationSpec};
use emerge_skia::tree::attrs::{
    Attrs, Background, BorderRadius, BorderWidth, BoxShadow, Color, Length, Padding,
};
use emerge_skia::tree::element::{Element, ElementKind, ElementTree, NodeId};
use emerge_skia::tree::layout::TextMeasurer;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const TEXT_ROW_COUNT: usize = 500;
pub const CARD_COUNT: usize = 160;
pub const SHADOW_RECIPE_CARD_COUNT: usize = 84;
pub const SCROLL_VIEWPORT_ROW_COUNT: usize = 2_000;

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

    let root_attrs = Attrs {
        width: Some(Length::Fill),
        spacing: Some(2.0),
        ..Default::default()
    };
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

            let row_attrs = Attrs {
                width: Some(Length::Fill),
                padding: Some(Padding::Uniform(2.0)),
                ..Default::default()
            };

            let text_attrs = Attrs {
                content: Some(format!(
                    "Benchmark row {index}: repeated text content for layout measurement"
                )),
                font_size: Some(16.0),
                ..Default::default()
            };

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

    let root_attrs = Attrs {
        width: Some(Length::Px(960.0)),
        spacing_x: Some(8.0),
        spacing_y: Some(8.0),
        ..Default::default()
    };
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

            let card_attrs = Attrs {
                width: Some(Length::Px(280.0)),
                padding: Some(Padding::Uniform(8.0)),
                spacing: Some(6.0),
                background: Some(Background::Color(Color::Rgb {
                    r: 245,
                    g: 247,
                    b: 250,
                })),
                border_radius: Some(BorderRadius::Uniform(8.0)),
                border_width: Some(BorderWidth::Uniform(1.0)),
                border_color: Some(Color::Rgb {
                    r: 220,
                    g: 226,
                    b: 235,
                }),
                ..Default::default()
            };

            let header_attrs = Attrs {
                width: Some(Length::Fill),
                spacing: Some(6.0),
                ..Default::default()
            };

            let title_attrs = Attrs {
                content: Some(format!("Card {index}")),
                font_size: Some(18.0),
                ..Default::default()
            };

            let badge_attrs = Attrs {
                content: Some(format!("#{index}")),
                font_size: Some(12.0),
                ..Default::default()
            };

            let body_attrs = Attrs {
                content: Some(
                    "Nested benchmark body copy with enough words to exercise text measurement."
                        .to_string(),
                ),
                font_size: Some(14.0),
                ..Default::default()
            };

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

pub fn large_simple_scroll_column(row_count: usize) -> ElementTree {
    large_scroll_column(row_count, ScrollViewportRowStyle::Simple)
}

pub fn large_paint_rich_scroll_column(row_count: usize) -> ElementTree {
    large_scroll_column(row_count, ScrollViewportRowStyle::PaintRich)
}

#[derive(Clone, Copy)]
enum ScrollViewportRowStyle {
    Simple,
    PaintRich,
}

fn large_scroll_column(row_count: usize, style: ScrollViewportRowStyle) -> ElementTree {
    let mut tree = ElementTree::new();
    let root_id = NodeId::from_u64(9_000_000);
    let content_id = NodeId::from_u64(9_000_001);
    tree.set_root_id(root_id);

    let root_attrs = Attrs {
        width: Some(Length::Px(900.0)),
        height: Some(Length::Px(640.0)),
        scrollbar_y: Some(true),
        background: Some(Background::Color(Color::Rgb {
            r: 244,
            g: 246,
            b: 250,
        })),
        ..Default::default()
    };
    let content_attrs = Attrs {
        width: Some(Length::Fill),
        padding: Some(Padding::Uniform(10.0)),
        spacing: Some(match style {
            ScrollViewportRowStyle::Simple => 2.0,
            ScrollViewportRowStyle::PaintRich => 8.0,
        }),
        ..Default::default()
    };

    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::El,
        Vec::new(),
        root_attrs,
    ));
    tree.insert(Element::with_attrs(
        content_id,
        ElementKind::Column,
        Vec::new(),
        content_attrs,
    ));

    let row_ids: Vec<_> = (0..row_count)
        .map(|index| match style {
            ScrollViewportRowStyle::Simple => insert_simple_scroll_row(&mut tree, index),
            ScrollViewportRowStyle::PaintRich => insert_paint_rich_scroll_row(&mut tree, index),
        })
        .collect();

    tree.set_children(&content_id, row_ids)
        .expect("scroll content children should exist");
    tree.set_children(&root_id, vec![content_id])
        .expect("scroll root child should exist");
    tree
}

fn insert_simple_scroll_row(tree: &mut ElementTree, index: usize) -> NodeId {
    let base = 9_100_000 + index as u64 * 10;
    let row_id = NodeId::from_u64(base);
    let text_id = NodeId::from_u64(base + 1);

    let row_attrs = Attrs {
        width: Some(Length::Fill),
        height: Some(Length::Px(34.0)),
        padding: Some(Padding::Sides {
            top: 6.0,
            right: 10.0,
            bottom: 6.0,
            left: 10.0,
        }),
        background: Some(Background::Color(if index.is_multiple_of(2) {
            Color::Rgb {
                r: 255,
                g: 255,
                b: 255,
            }
        } else {
            Color::Rgb {
                r: 238,
                g: 242,
                b: 247,
            }
        })),
        ..Default::default()
    };
    let text_attrs = Attrs {
        content: Some(format!("Viewport row {index:04}")),
        font_size: Some(14.0),
        font_color: Some(Color::Rgb {
            r: 31,
            g: 41,
            b: 55,
        }),
        ..Default::default()
    };

    tree.insert(Element::with_attrs(
        row_id,
        ElementKind::El,
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
        .expect("simple row child should exist");
    row_id
}

fn insert_paint_rich_scroll_row(tree: &mut ElementTree, index: usize) -> NodeId {
    let base = 9_500_000 + index as u64 * 20;
    let row_id = NodeId::from_u64(base);
    let header_id = NodeId::from_u64(base + 1);
    let title_id = NodeId::from_u64(base + 2);
    let badge_id = NodeId::from_u64(base + 3);
    let detail_id = NodeId::from_u64(base + 4);

    let hue = (index % 5) as u8;
    let row_attrs = Attrs {
        width: Some(Length::Fill),
        height: Some(Length::Px(82.0)),
        padding: Some(Padding::Uniform(10.0)),
        spacing: Some(5.0),
        background: Some(Background::Gradient {
            from: Color::Rgb {
                r: 246,
                g: 249,
                b: 255,
            },
            to: Color::Rgb {
                r: 232,
                g: 238 + hue,
                b: 248,
            },
            angle: 18.0,
        }),
        border_radius: Some(BorderRadius::Uniform(10.0)),
        border_width: Some(BorderWidth::Uniform(1.0)),
        border_color: Some(Color::Rgb {
            r: 205,
            g: 214,
            b: 228,
        }),
        box_shadows: Some(vec![BoxShadow {
            offset_x: 0.0,
            offset_y: 3.0,
            blur: 10.0,
            size: 0.0,
            color: Color::Rgba {
                r: 15,
                g: 23,
                b: 42,
                a: 32,
            },
            inset: false,
        }]),
        ..Default::default()
    };
    let header_attrs = Attrs {
        width: Some(Length::Fill),
        spacing: Some(6.0),
        ..Default::default()
    };
    let title_attrs = Attrs {
        content: Some(format!("Paint rich viewport row {index:04}")),
        font_size: Some(15.0),
        font_color: Some(Color::Rgb {
            r: 17,
            g: 24,
            b: 39,
        }),
        ..Default::default()
    };
    let badge_attrs = Attrs {
        content: Some(format!("cache {}", index % 17)),
        font_size: Some(12.0),
        font_color: Some(Color::Rgb {
            r: 79,
            g: 70,
            b: 229,
        }),
        ..Default::default()
    };
    let detail_attrs = Attrs {
        content: Some(
            "Extra paint-rich row copy with enough text to exercise clipping and text draw setup."
                .to_string(),
        ),
        font_size: Some(12.0),
        font_color: Some(Color::Rgb {
            r: 75,
            g: 85,
            b: 99,
        }),
        ..Default::default()
    };

    tree.insert(Element::with_attrs(
        row_id,
        ElementKind::Column,
        Vec::new(),
        row_attrs,
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
        detail_id,
        ElementKind::Text,
        Vec::new(),
        detail_attrs,
    ));
    tree.set_children(&header_id, vec![title_id, badge_id])
        .expect("paint rich row header children should exist");
    tree.set_children(&row_id, vec![header_id, detail_id])
        .expect("paint rich row children should exist");
    row_id
}

pub fn scrollable_animated_shadow_showcase() -> ElementTree {
    let mut tree = animated_shadow_showcase();
    let content_id = tree.root_id().expect("shadow showcase should have a root");
    let root_id = NodeId::from_u64(2);

    let root_attrs = Attrs {
        width: Some(Length::Px(960.0)),
        height: Some(Length::Px(640.0)),
        scrollbar_y: Some(true),
        background: Some(Background::Color(Color::Rgb {
            r: 241,
            g: 244,
            b: 250,
        })),
        ..Default::default()
    };

    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::El,
        Vec::new(),
        root_attrs,
    ));
    tree.set_children(&root_id, vec![content_id])
        .expect("scroll wrapper child should exist");
    tree.set_root_id(root_id);
    tree
}

pub fn animated_shadow_showcase() -> ElementTree {
    let mut tree = ElementTree::new();
    let root_id = NodeId::from_u64(3);
    let hero_id = NodeId::from_u64(4);
    let showcase_id = NodeId::from_u64(5);
    let grid_id = NodeId::from_u64(6);
    tree.set_root_id(root_id);

    let root_attrs = Attrs {
        width: Some(Length::Px(960.0)),
        padding: Some(Padding::Uniform(18.0)),
        spacing: Some(14.0),
        background: Some(Background::Color(Color::Rgb {
            r: 241,
            g: 244,
            b: 250,
        })),
        ..Default::default()
    };

    let hero_attrs = Attrs {
        width: Some(Length::Fill),
        padding: Some(Padding::Uniform(16.0)),
        spacing: Some(6.0),
        background: Some(Background::Color(Color::Rgb {
            r: 248,
            g: 250,
            b: 253,
        })),
        border_radius: Some(BorderRadius::Uniform(16.0)),
        border_width: Some(BorderWidth::Uniform(1.0)),
        border_color: Some(Color::Rgb {
            r: 223,
            g: 228,
            b: 238,
        }),
        ..Default::default()
    };

    let showcase_attrs = Attrs {
        width: Some(Length::Fill),
        padding: Some(Padding::Uniform(18.0)),
        spacing: Some(14.0),
        background: Some(Background::Color(Color::Rgb {
            r: 248,
            g: 250,
            b: 253,
        })),
        border_radius: Some(BorderRadius::Uniform(18.0)),
        box_shadows: Some(vec![shadow(0.0, 16.0, 28.0, 6.0, 46)]),
        ..Default::default()
    };

    let grid_attrs = Attrs {
        width: Some(Length::Fill),
        spacing_x: Some(12.0),
        spacing_y: Some(12.0),
        ..Default::default()
    };

    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::Column,
        Vec::new(),
        root_attrs,
    ));
    tree.insert(Element::with_attrs(
        hero_id,
        ElementKind::Column,
        Vec::new(),
        hero_attrs,
    ));
    tree.insert(Element::with_attrs(
        showcase_id,
        ElementKind::Row,
        Vec::new(),
        showcase_attrs,
    ));
    tree.insert(Element::with_attrs(
        grid_id,
        ElementKind::WrappedRow,
        Vec::new(),
        grid_attrs,
    ));

    let hero_children = vec![
        insert_text(
            &mut tree,
            10,
            "Directional, diffuse, and stacked shadows",
            20.0,
        ),
        insert_text(
            &mut tree,
            11,
            "Animated outer shadows are decorative paint and should not force layout.",
            13.0,
        ),
    ];
    tree.set_children(&hero_id, hero_children)
        .expect("hero children should exist");

    let showcase_children: Vec<_> = [
        animated_shadow_card(
            &mut tree,
            100,
            "Stacked",
            "Counter-rotating",
            Color::Rgb {
                r: 244,
                g: 248,
                b: 255,
            },
            stacked_shadow_animation(),
        ),
        animated_shadow_card(
            &mut tree,
            200,
            "Right cast",
            "Orbiting cast",
            Color::Rgb {
                r: 246,
                g: 243,
                b: 255,
            },
            orbiting_shadow_animation(14.0, 2.0, 14.0, 2400.0, false),
        ),
        animated_shadow_card(
            &mut tree,
            300,
            "Soft spread",
            "Orbiting blur",
            Color::Rgb {
                r: 240,
                g: 249,
                b: 246,
            },
            orbiting_shadow_animation(24.0, 6.0, 12.0, 3400.0, true),
        ),
    ]
    .into_iter()
    .collect();
    tree.set_children(&showcase_id, showcase_children)
        .expect("showcase children should exist");

    let recipe_children: Vec<_> = (0..SHADOW_RECIPE_CARD_COUNT)
        .map(|index| static_shadow_recipe_card(&mut tree, 1_000 + index as u64 * 10, index))
        .collect();
    tree.set_children(&grid_id, recipe_children)
        .expect("recipe children should exist");

    tree.set_children(&root_id, vec![hero_id, showcase_id, grid_id])
        .expect("root children should exist");
    tree
}

fn animated_shadow_card(
    tree: &mut ElementTree,
    base: u64,
    title: &str,
    subtitle: &str,
    background: Color,
    animation: AnimationSpec,
) -> NodeId {
    let card_id = NodeId::from_u64(base);
    let title_id = NodeId::from_u64(base + 1);
    let subtitle_id = NodeId::from_u64(base + 2);

    let attrs = Attrs {
        width: Some(Length::FillWeighted(1.0)),
        height: Some(Length::Px(94.0)),
        padding: Some(Padding::Uniform(14.0)),
        spacing: Some(4.0),
        background: Some(Background::Color(background)),
        border_radius: Some(BorderRadius::Uniform(14.0)),
        animate: Some(animation),
        ..Default::default()
    };

    tree.insert(Element::with_attrs(
        card_id,
        ElementKind::Column,
        Vec::new(),
        attrs,
    ));
    tree.insert(Element::with_attrs(
        title_id,
        ElementKind::Text,
        Vec::new(),
        text_attrs_with_size(title, 14.0),
    ));
    tree.insert(Element::with_attrs(
        subtitle_id,
        ElementKind::Text,
        Vec::new(),
        text_attrs_with_size(subtitle, 11.0),
    ));
    tree.set_children(&card_id, vec![title_id, subtitle_id])
        .expect("animated card children should exist");

    card_id
}

fn static_shadow_recipe_card(tree: &mut ElementTree, base: u64, index: usize) -> NodeId {
    let card_id = NodeId::from_u64(base);
    let sample_id = NodeId::from_u64(base + 1);
    let title_id = NodeId::from_u64(base + 2);
    let subtitle_id = NodeId::from_u64(base + 3);
    let detail_id = NodeId::from_u64(base + 4);

    let card_attrs = Attrs {
        width: Some(Length::Px(280.0)),
        padding: Some(Padding::Uniform(12.0)),
        spacing: Some(10.0),
        background: Some(Background::Color(Color::Rgb {
            r: 245,
            g: 247,
            b: 251,
        })),
        border_radius: Some(BorderRadius::Uniform(12.0)),
        border_width: Some(BorderWidth::Uniform(1.0)),
        border_color: Some(Color::Rgb {
            r: 223,
            g: 228,
            b: 238,
        }),
        ..Default::default()
    };

    let sample_attrs = Attrs {
        width: Some(Length::Fill),
        height: Some(Length::Px(84.0)),
        padding: Some(Padding::Uniform(12.0)),
        background: Some(Background::Color(Color::Rgb {
            r: 34,
            g: 38,
            b: 54,
        })),
        border_radius: Some(BorderRadius::Uniform(10.0)),
        box_shadows: Some(vec![shadow(
            (index % 5) as f64 - 2.0,
            8.0 + (index % 3) as f64,
            10.0 + (index % 4) as f64 * 2.0,
            (index % 2) as f64,
            80,
        )]),
        ..Default::default()
    };

    tree.insert(Element::with_attrs(
        card_id,
        ElementKind::Column,
        Vec::new(),
        card_attrs,
    ));
    tree.insert(Element::with_attrs(
        sample_id,
        ElementKind::El,
        Vec::new(),
        sample_attrs,
    ));
    tree.insert(Element::with_attrs(
        title_id,
        ElementKind::Text,
        Vec::new(),
        text_attrs_with_size(&format!("Shadow recipe {index}"), 13.0),
    ));
    tree.insert(Element::with_attrs(
        subtitle_id,
        ElementKind::Text,
        Vec::new(),
        text_attrs_with_size("Decorative paint-only shadow", 11.0),
    ));
    tree.insert(Element::with_attrs(
        detail_id,
        ElementKind::Text,
        Vec::new(),
        text_attrs_with_size("Border.shadow offset/blur/size/color", 10.0),
    ));
    tree.set_children(&card_id, vec![sample_id, title_id, subtitle_id, detail_id])
        .expect("recipe card children should exist");

    card_id
}

fn insert_text(tree: &mut ElementTree, id: u64, content: &str, font_size: f64) -> NodeId {
    let node_id = NodeId::from_u64(id);
    tree.insert(Element::with_attrs(
        node_id,
        ElementKind::Text,
        Vec::new(),
        text_attrs_with_size(content, font_size),
    ));
    node_id
}

fn text_attrs_with_size(content: &str, font_size: f64) -> Attrs {
    Attrs {
        content: Some(content.to_string()),
        font_size: Some(font_size),
        ..Default::default()
    }
}

fn stacked_shadow_animation() -> AnimationSpec {
    let primary = orbit_positions(12.0, false);
    let secondary = [
        (0.0, 8.0),
        (0.0, 8.0),
        (-8.0, 0.0),
        (-8.0, 0.0),
        (0.0, -8.0),
        (0.0, -8.0),
        (8.0, 0.0),
        (8.0, 0.0),
        (0.0, 8.0),
    ];

    let keyframes = primary
        .into_iter()
        .zip(secondary)
        .map(|((ax, ay), (bx, by))| Attrs {
            box_shadows: Some(vec![
                shadow(ax, ay, 18.0, 2.0, 41),
                shadow(bx, by, 10.0, 0.0, 89),
            ]),
            ..Default::default()
        })
        .collect();

    AnimationSpec {
        keyframes,
        duration_ms: 2800.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    }
}

fn orbiting_shadow_animation(
    blur: f64,
    size: f64,
    radius: f64,
    duration_ms: f64,
    counterclockwise: bool,
) -> AnimationSpec {
    let keyframes = orbit_positions(radius, counterclockwise)
        .into_iter()
        .map(|(x, y)| Attrs {
            box_shadows: Some(vec![shadow(x, y, blur, size, 67)]),
            ..Default::default()
        })
        .collect();

    AnimationSpec {
        keyframes,
        duration_ms,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    }
}

fn orbit_positions(radius: f64, counterclockwise: bool) -> Vec<(f64, f64)> {
    let positions = vec![
        (0.0, -radius),
        (radius, -radius),
        (radius, 0.0),
        (radius, radius),
        (0.0, radius),
        (-radius, radius),
        (-radius, 0.0),
        (-radius, -radius),
        (0.0, -radius),
    ];

    if counterclockwise {
        positions.into_iter().rev().collect()
    } else {
        positions
    }
}

fn shadow(offset_x: f64, offset_y: f64, blur: f64, size: f64, alpha: u8) -> BoxShadow {
    BoxShadow {
        offset_x,
        offset_y,
        blur,
        size,
        color: Color::Rgba {
            r: 15,
            g: 23,
            b: 42,
            a: alpha,
        },
        inset: false,
    }
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
