//! Shared resolved images. Only the drawing queue writes their contents or
//! changes their layout. The second queue reads TRANSFER_SRC_OPTIMAL; it
//! never executes graphics commands or borrows the drawing context.
use super::*;
use crate::session::wayland::textures::{self, Renderer};
use skia_safe::{
    ColorType,
    gpu::{SurfaceOrigin, backend_textures},
};
use std::sync::atomic::AtomicBool;

pub(super) fn memory(
    device: &VulkanDevice,
    req: vk::MemoryRequirements,
    flags: vk::MemoryPropertyFlags,
) -> Result<vk::DeviceMemory, String> {
    let types = unsafe {
        device
            .instance()
            .raw()
            .get_physical_device_memory_properties(device.physical_device())
    };
    let index = (0..types.memory_type_count)
        .find(|&i| {
            req.memory_type_bits & (1 << i) != 0
                && types.memory_types[i as usize]
                    .property_flags
                    .contains(flags)
        })
        .ok_or("no compatible Vulkan memory type")?;
    unsafe {
        device.raw().allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(index),
            None,
        )
    }
    .map_err(error)
}
pub(in crate::session::wayland) struct Backing {
    pub(super) image: vk::Image,
    allocation: vk::DeviceMemory,
    device: Arc<VulkanDevice>,
}
impl Drop for Backing {
    fn drop(&mut self) {
        unsafe {
            self.device.raw().destroy_image(self.image, None);
            self.device.raw().free_memory(self.allocation, None);
        }
    }
}
struct Semaphore {
    raw: vk::Semaphore,
    device: Arc<VulkanDevice>,
}
impl Drop for Semaphore {
    fn drop(&mut self) {
        unsafe {
            self.device.raw().destroy_semaphore(self.raw, None);
        }
    }
}
pub(in crate::session) struct Drawing {
    pub(super) src: Arc<Backing>,
    signal: Semaphore,
    waited: AtomicBool,
}
impl Drawing {
    /// Called only by the presentation queue owner. Later reads on that queue
    /// are in the second scope of this ALL_COMMANDS semaphore wait as well.
    pub(super) fn wait_once(&self) -> Option<vk::Semaphore> {
        (!self.waited.swap(true, std::sync::atomic::Ordering::AcqRel)).then_some(self.signal.raw)
    }
}
pub(super) struct DrawContext {
    ctx: GaneshContext,
    device: Arc<VulkanDevice>,
    family: u32,
}
impl DrawContext {
    pub fn new(device: Arc<VulkanDevice>) -> Result<Self, String> {
        let (queue, family) = device.drawing_queue().ok_or("no Vulkan drawing queue")?;
        let mut ctx = GaneshContext::on_queue(&device, queue, family)?;
        if ctx
            .direct_context_mut()?
            .max_surface_sample_count_for_color_type(ColorType::BGRA8888)
            < 4
        {
            return Err("BGRA8 4x hardware multisampling is unavailable".into());
        }
        Ok(Self {
            ctx,
            device,
            family,
        })
    }
}
impl Renderer for DrawContext {
    type Storage = Arc<Backing>;
    fn allocate(&mut self, (w, h): (u32, u32)) -> Result<(Surface, Self::Storage), String> {
        let d = &self.device;
        let families = [self.family, d.queue_family_index()];
        let split = families[0] != families[1];
        let usage = vk::ImageUsageFlags::TRANSFER_DST
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::COLOR_ATTACHMENT;
        let mut ci = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .extent(vk::Extent3D {
                width: w,
                height: h,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage);
        if split {
            ci = ci
                .sharing_mode(vk::SharingMode::CONCURRENT)
                .queue_family_indices(&families);
        }
        // SAFETY: the allocation and image are owned together. Both authorized
        // families are listed when distinct. Single-family queues share ownership.
        let mut b = Backing {
            device: d.clone(),
            image: unsafe { d.raw().create_image(&ci, None) }.map_err(error)?,
            allocation: vk::DeviceMemory::null(),
        };
        let req = unsafe { d.raw().get_image_memory_requirements(b.image) };
        b.allocation = memory(d, req, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
        unsafe { d.raw().bind_image_memory(b.image, b.allocation, 0) }.map_err(error)?;
        let mut info = unsafe {
            skvk::ImageInfo::new(
                raw::image_to_skia(b.image),
                skvk::Alloc::default(),
                skvk::ImageTiling::OPTIMAL,
                skvk::ImageLayout::UNDEFINED,
                skvk::Format::B8G8R8A8_UNORM,
                1,
                Some(vk::QUEUE_FAMILY_IGNORED),
                None,
                Some(gpu::Protected::No),
                Some(if split {
                    skvk::SharingMode::CONCURRENT
                } else {
                    skvk::SharingMode::EXCLUSIVE
                }),
            )
        };
        info.image_usage_flags = usage.as_raw();
        info.sample_count = 1;
        let texture = unsafe { backend_textures::make_vk((w as i32, h as i32), &info, "") };
        let surface = gpu::surfaces::wrap_backend_texture(
            self.ctx.direct_context_mut()?,
            &texture,
            SurfaceOrigin::TopLeft,
            4,
            ColorType::BGRA8888,
            None,
            None,
        )
        .ok_or("cannot wrap multisampled Vulkan draw target")?;
        Ok((surface, Arc::new(b)))
    }
    fn finish(&mut self, s: &mut Surface, b: &Self::Storage) -> Result<textures::Image, String> {
        let signal = Semaphore {
            raw: unsafe {
                self.device
                    .raw()
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
            }
            .map_err(error)?,
            device: self.device.clone(),
        };
        let state = skvk::mutable_texture_states::new_vulkan(
            skvk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            vk::QUEUE_FAMILY_IGNORED,
        );
        let mut sem = unsafe { backend_semaphores::make_vk(raw::semaphore_to_skia(signal.raw)) };
        let mut info = gpu::FlushInfo::default();
        unsafe { info.set_signal_semaphores(std::slice::from_mut(&mut sem)) };
        let ctx = self.ctx.direct_context_mut()?;
        if ctx.flush_surface_with_texture_state(s, &info, Some(&state))
            != Ok(gpu::SemaphoresSubmitted::Yes)
            || !ctx.submit(gpu::SyncCpu::Yes)
            || ctx.abandoned()
        {
            return Err("Vulkan draw/resolve failed".into());
        }
        Ok(textures::Image::Vk(Drawing {
            src: b.clone(),
            signal,
            waited: AtomicBool::new(false),
        }))
    }
}
