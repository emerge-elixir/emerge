use skia_safe::{
    AlphaType, ColorType, ImageInfo, Surface,
    gpu::{self, SurfaceOrigin, backend_render_targets, gl::FramebufferInfo},
};

use crate::renderer::{RenderFrame, text_surface_props};

pub struct GlFrameSurface {
    surface: Surface,
    direct_context: gpu::DirectContext,
    fb_info: FramebufferInfo,
    num_samples: usize,
    stencil_size: usize,
}

impl GlFrameSurface {
    pub fn new(
        dimensions: (u32, u32),
        fb_info: FramebufferInfo,
        direct_context: gpu::DirectContext,
        num_samples: usize,
        stencil_size: usize,
    ) -> Self {
        let mut direct_context = direct_context;
        let surface = create_gl_surface(
            (dimensions.0 as i32, dimensions.1 as i32),
            fb_info,
            &mut direct_context,
            num_samples,
            stencil_size,
        );

        Self {
            surface,
            direct_context,
            fb_info,
            num_samples,
            stencil_size,
        }
    }

    pub fn resize(&mut self, dimensions: (u32, u32)) {
        self.surface = create_gl_surface(
            (dimensions.0 as i32, dimensions.1 as i32),
            self.fb_info,
            &mut self.direct_context,
            self.num_samples,
            self.stencil_size,
        );
    }

    pub fn frame(&mut self) -> RenderFrame<'_> {
        RenderFrame::new(&mut self.surface, Some(&mut self.direct_context))
    }

    pub fn capture_rgba_pixels(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        let size = self.surface.image_info().dimensions();
        let width = u32::try_from(size.width).ok()?;
        let height = u32::try_from(size.height).ok()?;
        let row_bytes = usize::try_from(width).ok()?.checked_mul(4)?;
        let mut pixels = vec![0_u8; row_bytes.checked_mul(usize::try_from(height).ok()?)?];
        let info = ImageInfo::new(
            (size.width, size.height),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );

        self.direct_context.flush_and_submit();
        self.surface
            .read_pixels(&info, pixels.as_mut_slice(), row_bytes, (0, 0));

        Some((width, height, pixels))
    }
}

impl Drop for GlFrameSurface {
    fn drop(&mut self) {
        self.direct_context.flush_and_submit();
        self.direct_context
            .perform_deferred_cleanup(std::time::Duration::ZERO, None);
        self.direct_context.free_gpu_resources();
        self.direct_context.flush_and_submit();

        #[cfg(not(test))]
        skia_safe::graphics::purge_all_caches();
    }
}

fn create_gl_surface(
    dimensions: (i32, i32),
    fb_info: FramebufferInfo,
    direct_context: &mut gpu::DirectContext,
    num_samples: usize,
    stencil_size: usize,
) -> Surface {
    let backend_render_target =
        backend_render_targets::make_gl(dimensions, num_samples, stencil_size, fb_info);

    let surface_props = text_surface_props();

    gpu::surfaces::wrap_backend_render_target(
        direct_context,
        &backend_render_target,
        SurfaceOrigin::BottomLeft,
        skia_safe::ColorType::RGBA8888,
        None,
        Some(&surface_props),
    )
    .expect("Could not create Skia surface")
}
