//! Owned CPU video frames for external hosts.

use std::{collections::HashSet, sync::Arc};

use crate::{CleanupDispatcher, renderer::SceneRenderer, video};

pub struct CpuVideoFrames(Arc<video::VideoRegistry>);

pub struct CpuVideoSubmission {
    pub target: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl CpuVideoSubmission {
    pub fn decode(payload: &[u8]) -> Result<Self, String> {
        let invalid = || "invalid macOS binary video frame".to_string();
        if payload.len() < 11 || payload.len() > 64 * 1024 * 1024 {
            return Err(invalid());
        }
        let name_len = u16::from_be_bytes(payload[..2].try_into().unwrap()) as usize;
        if name_len == 0 || payload.len() < name_len + 11 {
            return Err(invalid());
        }
        let target = std::str::from_utf8(&payload[2..2 + name_len])
            .map_err(|_| invalid())?
            .to_owned();
        let header = &payload[2 + name_len..];
        let width = u32::from_be_bytes(header[..4].try_into().unwrap());
        let height = u32::from_be_bytes(header[4..8].try_into().unwrap());
        let alpha = header[8];
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4));
        if width == 0 || height == 0 || alpha > 2 || expected != Some(header.len() - 9) {
            return Err(invalid());
        }
        let rgba = header[9..]
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| match alpha {
                1 => [premultiply(r, a), premultiply(g, a), premultiply(b, a), a],
                2 => [r, g, b, 255],
                _ => [r, g, b, a],
            })
            .collect();
        Ok(Self {
            target,
            width,
            height,
            rgba,
        })
    }
}

fn premultiply(channel: u8, alpha: u8) -> u8 {
    ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8
}

impl CpuVideoFrames {
    pub fn new() -> Result<Self, String> {
        let release = video::spawn_release_worker().map_err(|error| error.to_string())?;
        Ok(Self(Arc::new(video::VideoRegistry::new(
            release,
            CleanupDispatcher::start()?,
            None,
        ))))
    }

    pub fn submit(
        &self,
        target: &str,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), String> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4));
        if width == 0
            || height == 0
            || width > i32::MAX as u32
            || height > i32::MAX as u32
            || expected != Some(rgba.len())
        {
            return Err("invalid RGBA video dimensions or byte length".to_string());
        }
        self.0.submit_cpu_frame(
            target,
            video::CpuVideoFrame {
                generation: 0,
                width,
                height,
                rgba: rgba.into(),
            },
        )?;
        Ok(())
    }

    pub fn sync(
        &self,
        renderer: &mut SceneRenderer,
        targets: &HashSet<String>,
    ) -> Result<bool, String> {
        self.0.set_active_targets(targets)?;
        renderer.sync_cpu_video_frames(&self.0)
    }
}

impl Drop for CpuVideoFrames {
    fn drop(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        render_scene::{DrawPrimitive, RenderNode, RenderScene},
        renderer::{RenderFrame, RenderState},
        tree::attrs::ImageFit,
    };
    use skia_safe::{AlphaType, Color, ColorType, ImageInfo, surfaces};

    fn payload(alpha: u8) -> Vec<u8> {
        [
            vec![0, 1, b'v'],
            1_u32.to_be_bytes().to_vec(),
            1_u32.to_be_bytes().to_vec(),
            vec![alpha, 200, 100, 50, 128],
        ]
        .concat()
    }

    #[test]
    fn wire_frames_validate_lengths_and_normalize_alpha() {
        assert_eq!(
            CpuVideoSubmission::decode(&payload(0)).unwrap().rgba,
            [200, 100, 50, 128]
        );
        assert_eq!(
            CpuVideoSubmission::decode(&payload(1)).unwrap().rgba,
            [100, 50, 25, 128]
        );
        assert_eq!(
            CpuVideoSubmission::decode(&payload(2)).unwrap().rgba,
            [200, 100, 50, 255]
        );
        assert!(CpuVideoSubmission::decode(&payload(3)).is_err());
        let bytes = payload(0);
        for length in 0..bytes.len() {
            assert!(CpuVideoSubmission::decode(&bytes[..length]).is_err());
        }
        let mut oversized = bytes.clone();
        oversized[3..7].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(CpuVideoSubmission::decode(&oversized).is_err());
        let mut extra = bytes;
        extra.push(0);
        assert!(CpuVideoSubmission::decode(&extra).is_err());
    }

    #[test]
    fn cpu_video_replaces_visible_pixels_and_discards_hidden_frames() {
        let frames = CpuVideoFrames::new().unwrap();
        let mut renderer = SceneRenderer::with_cache_config(Default::default());
        let state = RenderState::new(
            RenderScene {
                nodes: vec![RenderNode::Primitive(DrawPrimitive::Video(
                    0.0,
                    0.0,
                    2.0,
                    2.0,
                    "v".into(),
                    ImageFit::Contain,
                ))],
                ..Default::default()
            },
            Color::TRANSPARENT,
            1,
            false,
        );
        let mut surface = surfaces::raster_n32_premul((2, 2)).unwrap();
        let mut pixels = [0_u8; 16];
        let info = ImageInfo::new((2, 2), ColorType::RGBA8888, AlphaType::Premul, None);
        frames.sync(&mut renderer, &state.video_target_ids).unwrap();
        for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            frames.submit("v", 1, 1, color.to_vec()).unwrap();
            assert!(frames.sync(&mut renderer, &state.video_target_ids).unwrap());
            renderer.render(&mut RenderFrame::new(&mut surface, None), &state);
            assert!(surface.read_pixels(&info, &mut pixels, 8, (0, 0)));
            assert_eq!(pixels, color.repeat(4).as_slice());
        }
        frames.sync(&mut renderer, &HashSet::new()).unwrap();
        frames.submit("v", 1, 1, vec![0, 255, 0, 255]).unwrap();
        frames.sync(&mut renderer, &state.video_target_ids).unwrap();
        renderer.render(&mut RenderFrame::new(&mut surface, None), &state);
        assert!(surface.read_pixels(&info, &mut pixels, 8, (0, 0)));
        assert_eq!(pixels, [0; 16]);
    }
}
