//! Validate the authored, static SVG subset *before* usvg can discard unknown
//! nodes. Count referenced subtrees with their multiplicity before expanding
//! <use>, patterns and masks; checking just the XML depth is insufficient.
use std::collections::HashMap;
use usvg::roxmltree::{Document, Node, NodeId};

const SVG: &str = "http://www.w3.org/2000/svg";
pub(super) const NODES: u32 = 8192;
pub(super) const DEPTH: u32 = 64;
pub(super) const WORK: usize = 131_072;
const FILTER_CHAIN: usize = 8;

/// Bound depth before roxmltree's recursive element parser. This streaming
/// guard builds no second XML tree. Include namespace declarations in the
/// attribute limit, before duplicate checks and namespace resolution.
pub(super) fn framing(xml: &str) -> Option<()> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let (mut depth, mut attrs, mut elements) = (0_u32, 0, 0);
    loop {
        let event = reader.read_event().ok()?;
        match &event {
            Event::Start(tag) | Event::Empty(tag) => {
                let n = tag.attributes().with_checks(false).take(65).count();
                attrs += n;
                elements += 1;
                if n > 64 || attrs > 32_768 || elements > NODES || depth >= DEPTH {
                    return None;
                }
                depth += u32::from(matches!(event, Event::Start(_)));
            }
            Event::End(_) => depth = depth.checked_sub(1)?,
            Event::DocType(_) | Event::PI(_) => return None,
            Event::Eof => return (depth == 0).then_some(()),
            _ => (),
        }
    }
}

pub(super) fn check(doc: &Document<'_>) -> Option<usize> {
    let root = doc.root_element();
    if !root.has_tag_name((SVG, "svg")) {
        return None;
    }
    let view_box = root
        .attribute("viewBox")
        .map(str::parse::<svgtypes::ViewBox>)
        .transpose()
        .ok()?;
    if let Some(v) = view_box {
        usvg::NonZeroRect::from_xywh(v.x as f32, v.y as f32, v.w as f32, v.h as f32)?;
    }
    if let Some(aspect) = root.attribute("preserveAspectRatio") {
        aspect.parse::<svgtypes::AspectRatio>().ok()?;
    }
    for axis in ["width", "height"] {
        match root.attribute(axis) {
            Some(s) => {
                let length: svgtypes::Length = s.parse().ok()?;
                use svgtypes::LengthUnit::*;
                if !matches!(length.unit, None | Px | In | Cm | Mm | Pt | Pc)
                    || !length.number.is_finite()
                    || length.number <= 0.
                {
                    return Option::None;
                }
            }
            None if view_box.is_none() => return None,
            _ => (),
        }
    }
    let mut ids = HashMap::new();
    for n in doc.descendants().filter(Node::is_element) {
        if let Some(id) = n.attribute("id")
            && (id.is_empty() || ids.insert(id, n).is_some())
        {
            return None;
        }
    }
    let mut costs = HashMap::new();
    visit(root, &ids, &mut costs, 0).map(|c| c.0)
}

type Cost = (usize, u32);
fn visit<'d, 'i>(
    node: Node<'d, 'i>,
    ids: &HashMap<&str, Node<'d, 'i>>,
    memo: &mut HashMap<NodeId, Option<Cost>>,
    level: u32,
) -> Option<Cost> {
    if level > DEPTH {
        return None;
    }
    if let Some(c) = memo.get(&node.id()) {
        return c.filter(|(_, h)| level + h <= DEPTH);
    }
    memo.insert(node.id(), None); // gray: a reference to an ancestor is a cycle
    let tag = node.tag_name().name();
    if node.tag_name().namespace() != Some(SVG)
        || !matches!(
            tag,
            "svg"
                | "g"
                | "defs"
                | "symbol"
                | "use"
                | "path"
                | "rect"
                | "circle"
                | "ellipse"
                | "line"
                | "polyline"
                | "polygon"
                | "linearGradient"
                | "radialGradient"
                | "stop"
                | "pattern"
                | "clipPath"
                | "mask"
                | "filter"
                | "feColorMatrix"
                | "text"
                | "tspan"
                | "title"
                | "desc"
        )
    {
        return None;
    }
    if tag == "filter" {
        let mut results = [""; FILTER_CHAIN];
        let mut empty = true;
        for (i, n) in node.children().filter(Node::is_element).enumerate() {
            if i >= FILTER_CHAIN || !n.has_tag_name((SVG, "feColorMatrix")) {
                return None;
            }
            if let Some(input) = n.attribute("in")
                && !matches!(input, "SourceGraphic" | "SourceAlpha")
                && (input.is_empty() || !results[..i].contains(&input))
            {
                return None;
            }
            results[i] = n.attribute("result").unwrap_or("");
            empty = false;
        }
        if empty {
            return None;
        }
    }
    if tag == "feColorMatrix" {
        if node.attribute("type").unwrap_or("matrix") != "matrix" {
            return None;
        }
        let mut values = svgtypes::NumberListParser::from(node.attribute("values")?);
        if values.try_fold(0, |n, v| {
            v.ok()
                .filter(|x| x.is_finite() && x.abs() <= 1024.)
                .map(|_| n + 1)
        })? != 20
        {
            return None;
        }
    }
    let mut cost: Cost = (1, 0);
    let mut inherited = 0;
    let mut reference = |id: &str, kind: &str| {
        let n = *ids.get(id)?;
        let target = n.tag_name().name();
        let supported = match kind {
            "use" => matches!(
                target,
                "svg"
                    | "g"
                    | "symbol"
                    | "use"
                    | "path"
                    | "rect"
                    | "circle"
                    | "ellipse"
                    | "line"
                    | "polyline"
                    | "polygon"
                    | "text"
            ),
            "fill" | "stroke" => matches!(target, "linearGradient" | "radialGradient" | "pattern"),
            "linearGradient" | "radialGradient" => {
                matches!(target, "linearGradient" | "radialGradient")
            }
            "clip-path" => target == "clipPath",
            _ => target == kind,
        };
        if !supported {
            return None;
        }
        let (work, height) = visit(n, ids, memo, level + 1)?;
        cost.0 += work;
        if matches!(kind, "fill" | "stroke") {
            inherited += work;
        }
        cost.1 = cost.1.max(1 + height);
        (cost.0 <= WORK).then_some(())
    };
    let mut local = 0;
    for a in node.attributes() {
        let (name, v) = (a.name(), a.value().trim());
        if a.namespace().is_some()
            && !(name == "href" && a.namespace() == Some("http://www.w3.org/1999/xlink"))
            && !(name == "space" && a.namespace() == Some("http://www.w3.org/XML/1998/namespace"))
        {
            return None;
        }
        if name == "d" {
            for segment in svgtypes::PathParser::from(v) {
                segment.ok()?;
                local += 1;
                if local > WORK {
                    return None;
                }
            }
        } else if v.len() > 4096 {
            return None;
        }
        match name {
            "href" => {
                if !matches!(tag, "use" | "linearGradient" | "radialGradient" | "pattern") {
                    return None;
                }
                reference(v.strip_prefix('#')?, tag)?;
            }
            "style" => {
                for decl in v.split(';').filter(|s| !s.trim().is_empty()) {
                    let (name, value) = decl.split_once(':')?;
                    if !presentation(name.trim()) {
                        return None;
                    }
                    if let Some(id) = paint_value(name.trim(), value.trim())? {
                        reference(id, name.trim())?;
                    }
                }
            }
            name if presentation(name) => {
                if let Some(id) = paint_value(name, v)? {
                    reference(id, name)?;
                }
            }
            "d"
            | "id"
            | "space"
            | "version"
            | "viewBox"
            | "preserveAspectRatio"
            | "width"
            | "height"
            | "x"
            | "y"
            | "dx"
            | "dy"
            | "x1"
            | "x2"
            | "y1"
            | "y2"
            | "cx"
            | "cy"
            | "r"
            | "rx"
            | "ry"
            | "fx"
            | "fy"
            | "fr"
            | "offset"
            | "transform"
            | "gradientUnits"
            | "gradientTransform"
            | "spreadMethod"
            | "clipPathUnits"
            | "maskUnits"
            | "maskContentUnits"
            | "patternUnits"
            | "patternContentUnits"
            | "patternTransform"
            | "filterUnits"
            | "primitiveUnits"
            | "in"
            | "result"
            | "type"
            | "values" => (),
            "points" => {
                local += v.len();
            }
            _ => return None,
        }
    }
    for n in node.children() {
        if n.is_element() {
            // An inherited paint server may be resolved for each bounding box.
            let (work, height) = visit(n, ids, memo, level + 1)?;
            cost.0 = cost.0.checked_add(work.checked_mul(1 + inherited)?)?;
            cost.1 = cost.1.max(1 + height);
            if cost.0 > WORK {
                return None;
            }
        } else if n.is_text() && matches!(tag, "text" | "tspan") {
            local += 128 * n.text()?.len();
        }
    }
    cost.0 += local;
    if cost.0 > WORK {
        return None;
    }
    memo.insert(node.id(), Some(cost));
    Some(cost)
}
fn presentation(name: &str) -> bool {
    matches!(
        name,
        "fill"
            | "fill-opacity"
            | "fill-rule"
            | "stroke"
            | "stroke-opacity"
            | "stroke-linejoin"
            | "stroke-linecap"
            | "stroke-miterlimit"
            | "stroke-width"
            | "stroke-dasharray"
            | "stroke-dashoffset"
            | "opacity"
            | "color"
            | "clip-path"
            | "clip-rule"
            | "mask"
            | "mask-type"
            | "filter"
            | "color-interpolation-filters"
            | "stop-color"
            | "stop-opacity"
            | "font-family"
            | "font-size"
            | "font-style"
            | "font-weight"
            | "text-anchor"
            | "letter-spacing"
            | "word-spacing"
            | "display"
            | "visibility"
    )
}
fn paint_value<'a>(name: &str, v: &'a str) -> Option<Option<&'a str>> {
    if v.contains(['\\', '@', ';', '!', '/']) {
        return None;
    }
    if let Some(id) = v.strip_prefix("url(#").and_then(|s| s.strip_suffix(')')) {
        return (!id.is_empty()
            && !id.contains(['(', ')'])
            && matches!(name, "fill" | "stroke" | "filter" | "clip-path" | "mask"))
        .then_some(Some(id));
    }
    if name == "mask-type" && !matches!(v, "alpha" | "luminance")
        || name == "color-interpolation-filters" && !matches!(v, "sRGB" | "linearRGB" | "auto")
        || matches!(name, "filter" | "mask" | "clip-path") && v != "none"
        || v.to_ascii_lowercase().contains("url")
    {
        return None;
    }
    Some(None)
}
