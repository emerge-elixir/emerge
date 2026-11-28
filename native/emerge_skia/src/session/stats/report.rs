use super::*;
use std::fmt::Write;

fn line(out: &mut String, label: &str, t: DurationStatsNif) {
    if t.count == 0 {
        writeln!(out, "    {label}: no samples (count=0)").unwrap();
    } else {
        writeln!(
            out,
            "    {label}: avg={:.3} ms min={:.3} ms max={:.3} ms count={}",
            t.avg_ms, t.min_ms, t.max_ms, t.count
        )
        .unwrap();
    }
}
pub(super) fn format(owner: &Owner, r: &Reading) -> String {
    let api = owner.selected.get().unwrap_or(&Selected::Raster).name();
    let requested = &owner.api;
    let api_label = if api == requested {
        api.to_string()
    } else {
        format!("{api} (requested {requested})")
    };
    let label = if owner.backend == "headless" {
        "target cadence"
    } else {
        "display mode"
    };
    let f = r.frames();
    let mut out = format!(
        "renderer stats\n  window\n    backend: {}\n    rendering_api: {api_label}\n    duration: {} ms\n    frames: {}\n    fps: {:.1}\n",
        owner.backend,
        r.elapsed.as_millis(),
        f.frame_count,
        f.fps
    );
    if r.interval.is_some() {
        writeln!(
            out,
            "    {label}: {:.1} fps ({:.3} ms/frame)",
            f.display_fps, f.display_frame_ms
        )
        .unwrap();
    } else {
        writeln!(out, "    {label}: unknown").unwrap();
    }
    out.push_str("\n  timings\n");
    for (metric, name, value) in r.counts.timings.rows() {
        let unavailable = match metric {
            Metric::Flush | Metric::GpuFlush | Metric::Submit if api == "raster" => {
                Some("raster output")
            }
            Metric::CallbackTotal | Metric::CallbackWait if owner.backend != "wayland" => {
                Some("no Wayland frame callback")
            }
            _ => None,
        };
        if let Some(why) = unavailable {
            writeln!(out, "    {name}: unavailable ({why})").unwrap();
        } else {
            line(&mut out, name, value);
        }
    }
    for (label, sample) in ["edit decode", "edit prepare", "edit apply"]
        .into_iter()
        .zip(r.counts.edits)
    {
        line(&mut out, label, sample);
    }
    let [frames, runs, parts] = r.layout;
    let [commands, glyphs, hits] = r.packed;
    let p = r.counts.pipeline;
    writeln!(out, "\n  layout cache\n    retained entries at last seal: frames={frames} glyph_runs={runs} compositions={parts} (not hit/miss counts)").unwrap();
    writeln!(out, "\n  scene pipeline\n    constructed={} queue_overwrites={}\n    last sealed: commands={commands} glyphs={glyphs} hits={hits}", p.scenes_constructed, p.render_queue_overwrites).unwrap();
    writeln!(
        out,
        "\n  video pipeline\n    unavailable on retained {} (no video, DRM or GPU elapsed samples)",
        owner.backend
    )
    .unwrap();
    writeln!(
        out,
        "\n  renderer cache\n    paint_layer: unavailable; reused installed drawing targets={}",
        r.counts.reused
    )
    .unwrap();
    out.push_str("\n  asset memory\n    ");
    if let Some(assets) = owner.stats.0.as_ref().and_then(|s| s.assets.get()) {
        write!(out, "{assets}").unwrap();
    } else {
        out.push_str("asset loader not configured");
    }
    out
}
