//! Geometry only. Never call Skia's bitmap/atlas glyph renderer on a GPU session.
use super::*;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
    math::point,
    path::{Path, path::Builder},
};
use skia_safe::{BlendMode, Canvas, Vertices, vertices};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::Size,
    outline::{DrawSettings, OutlinePen},
};
use std::collections::HashMap;

struct Pen(Builder);
impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.begin(point(x, -y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(point(x, -y));
    }
    fn quad_to(&mut self, x: f32, y: f32, x1: f32, y1: f32) {
        self.0.quadratic_bezier_to(point(x, -y), point(x1, -y1));
    }
    fn curve_to(&mut self, x: f32, y: f32, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.0
            .cubic_bezier_to(point(x, -y), point(x1, -y1), point(x2, -y2));
    }
    fn close(&mut self) {
        self.0.close();
    }
}
enum Outline {
    Mesh(Vertices),
    Synthetic(skia_safe::Path),
}
fn mesh(face: text::Face, size: f32, id: u16) -> Result<Option<Outline>, String> {
    // Synthetic embolden/skew changes the outline, not just the advance.
    if face.synthetic() {
        return Ok(face.font(size).get_path(id).map(Outline::Synthetic));
    }
    let font = FontRef::new(face.bytes()).map_err(|e| e.to_string())?;
    let outline = font
        .outline_glyphs()
        .get(GlyphId::new(id.into()))
        .ok_or("glyph has no vector outline")?;
    let mut pen = Pen(Path::builder());
    outline
        .draw(DrawSettings::from(Size::new(size)), &mut pen)
        .map_err(|e| e.to_string())?;
    let mut buf = VertexBuffers::<Point, u16>::new();
    FillTessellator::new()
        .tessellate_path(
            &pen.0.build(),
            &FillOptions::default()
                .with_fill_rule(FillRule::NonZero)
                .with_tolerance(0.12),
            &mut BuffersBuilder::new(&mut buf, |v: FillVertex| {
                Point::new(v.position().x, v.position().y)
            }),
        )
        .map_err(|e| format!("glyph geometry: {e:?}"))?;
    if buf.indices.is_empty() {
        return Ok(None);
    }
    let mut builder = vertices::Builder::new(
        vertices::VertexMode::Triangles,
        buf.vertices.len(),
        buf.indices.len(),
        vertices::BuilderFlags::empty(),
    );
    builder.positions().copy_from_slice(&buf.vertices);
    builder
        .indices()
        .ok_or("missing triangle indices")?
        .copy_from_slice(&buf.indices);
    Ok(Some(Outline::Mesh(builder.detach())))
}
#[derive(Default)]
pub(crate) struct GeometryPainter {
    meshes: HashMap<(text::Face, u32, u16), Option<Outline>>,
}
impl GeometryPainter {
    pub(crate) fn record(&mut self, canvas: &Canvas, raster: &Raster) -> Result<(), String> {
        replay(
            canvas,
            raster.tree.output.drawing(),
            |canvas, run, paint| {
                for (&id, p) in run.ids.iter().zip(run.positions) {
                    let key = (run.face, run.size.to_bits(), id);
                    if !self.meshes.contains_key(&key) {
                        let m = mesh(run.face, run.size, id)?;
                        if self.meshes.len() >= 256 {
                            self.meshes.clear();
                        }
                        self.meshes.insert(key, m);
                    }
                    if let Some(m) = &self.meshes[&key] {
                        canvas.save();
                        let (x, y) = (run.origin[0] + p.x, run.origin[1] + p.y);
                        canvas.translate((x, y));
                        let anchored = paint.shader().map(|shader| {
                            let mut local = paint.clone();
                            local.set_shader(
                                shader.with_local_matrix(&skia_safe::Matrix::translate((-x, -y))),
                            );
                            local
                        });
                        let paint = anchored.as_ref().unwrap_or(paint);
                        match m {
                            Outline::Mesh(v) => {
                                canvas.draw_vertices(v, BlendMode::SrcOver, paint);
                            }
                            Outline::Synthetic(path) => {
                                canvas.draw_path(path, paint);
                            }
                        }
                        canvas.restore();
                    }
                }
                Ok(())
            },
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spaces_and_card_labels_need_only_outline_geometry() {
        for f in [Family::Sans, Family::Mono].map(text::Face::from) {
            let font = FontRef::new(f.bytes()).unwrap();
            assert!(
                mesh(f, 16., u16::MAX).is_err(),
                "a missing outline is not an empty or bitmap glyph"
            );
            for c in " Moves:123Coral".chars() {
                let id = font.charmap().map(c).unwrap();
                assert_eq!(
                    mesh(f, 16., id.to_u32() as u16).unwrap().is_some(),
                    c != ' '
                );
            }
        }
    }
}
