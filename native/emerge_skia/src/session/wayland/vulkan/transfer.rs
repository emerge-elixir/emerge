//! The WSI queue never sees Ganesh's draw commands. A claim costs one GPU
//! image copy. Ordinary handoffs never allocate or map a host pixel buffer.
use super::*;
use crate::session::{
    capture::Rect,
    wayland::{Lease, textures},
};
struct Buffer {
    raw: vk::Buffer,
    allocation: vk::DeviceMemory,
    len: usize,
    device: Arc<VulkanDevice>,
}
impl Buffer {
    fn readback(device: &Arc<VulkanDevice>, len: usize) -> Result<Self, String> {
        let ci = vk::BufferCreateInfo::default()
            .size(len as u64)
            .usage(vk::BufferUsageFlags::TRANSFER_DST);
        let raw = unsafe { device.raw().create_buffer(&ci, None) }.map_err(error)?;
        let mut b = Self {
            raw,
            allocation: vk::DeviceMemory::null(),
            len,
            device: device.clone(),
        };
        let req = unsafe { device.raw().get_buffer_memory_requirements(raw) };
        b.allocation = targets::memory(
            device,
            req,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        unsafe { device.raw().bind_buffer_memory(raw, b.allocation, 0) }.map_err(error)?;
        Ok(b)
    }
    fn rgba(&self) -> Result<Vec<u8>, String> {
        let mut rgba = vec![0; self.len];
        // SAFETY: fence proven complete, host barrier recorded, coherent
        // allocation covers len and remains mapped for exactly this copy.
        unsafe {
            let p = self
                .device
                .raw()
                .map_memory(
                    self.allocation,
                    0,
                    self.len as u64,
                    vk::MemoryMapFlags::empty(),
                )
                .map_err(error)?;
            rgba.copy_from_slice(std::slice::from_raw_parts(p.cast(), self.len));
            self.device.raw().unmap_memory(self.allocation);
        }
        rgba.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
        Ok(rgba)
    }
}
impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            self.device.raw().destroy_buffer(self.raw, None);
            self.device.raw().free_memory(self.allocation, None);
        }
    }
}
impl Lease {
    fn vk_drawing(&self) -> Result<&Drawing, &'static str> {
        match &self.image {
            textures::Image::Vk(drawing) => Ok(drawing),
            #[cfg(feature = "opengl")]
            textures::Image::Gl(_) => Err("not a Vulkan image"),
        }
    }
}
fn layers() -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .layer_count(1)
}
fn range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1)
}
fn barrier(
    image: vk::Image,
    old: vk::ImageLayout,
    new: vk::ImageLayout,
) -> vk::ImageMemoryBarrier<'static> {
    vk::ImageMemoryBarrier::default()
        .image(image)
        .old_layout(old)
        .new_layout(new)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .subresource_range(range())
}

pub(super) struct Transfer {
    device: Arc<VulkanDevice>,
    pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    pending: Option<(Arc<Lease>, Option<Buffer>)>,
}
impl Drop for Transfer {
    fn drop(&mut self) {
        debug_assert!(self.pending.is_none());
        unsafe {
            self.device.raw().destroy_fence(self.fence, None);
            self.device.raw().destroy_command_pool(self.pool, None);
        }
    }
}
impl Transfer {
    pub fn new(device: &Arc<VulkanDevice>) -> Result<Self, String> {
        let info =
            vk::CommandPoolCreateInfo::default().queue_family_index(device.queue_family_index());
        let pool = unsafe { device.raw().create_command_pool(&info, None) }.map_err(error)?;
        let mut t = Self {
            device: device.clone(),
            pool,
            cmd: vk::CommandBuffer::null(),
            fence: vk::Fence::null(),
            pending: None,
        };
        let ai = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        unsafe {
            t.cmd = device.raw().allocate_command_buffers(&ai).map_err(error)?[0];
            t.fence = device
                .raw()
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .map_err(error)?;
        }
        Ok(t)
    }
    pub fn retire(&mut self) -> Result<(), String> {
        // Never use device_wait_idle here: another thread owns the draw queue.
        unsafe { self.device.raw().queue_wait_idle(self.device.queue()) }.map_err(error)?;
        self.pending.take();
        Ok(())
    }
    pub fn copy(
        &mut self,
        lease: &Arc<Lease>,
        dst: vk::Image,
        signal: vk::Semaphore,
    ) -> Result<(), String> {
        let (w, h) = lease.size;
        self.execute(lease, Some(signal), None, |d, cmd, src| unsafe {
            let layout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
            let stage = vk::PipelineStageFlags::TRANSFER;
            let write = vk::AccessFlags::TRANSFER_WRITE;
            let prepare = barrier(dst, vk::ImageLayout::UNDEFINED, layout).dst_access_mask(write);
            d.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                stage,
                Default::default(),
                &[],
                &[],
                &[prepare],
            );
            let copy = vk::ImageCopy::default()
                .src_subresource(layers())
                .dst_subresource(layers())
                .extent(vk::Extent3D {
                    width: w,
                    height: h,
                    depth: 1,
                });
            d.cmd_copy_image(
                cmd,
                src,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                dst,
                layout,
                &[copy],
            );
            let release =
                barrier(dst, layout, vk::ImageLayout::PRESENT_SRC_KHR).src_access_mask(write);
            d.cmd_pipeline_barrier(
                cmd,
                stage,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                Default::default(),
                &[],
                &[],
                &[release],
            );
        })?;
        Ok(())
    }
    pub fn read(&mut self, lease: &Arc<Lease>, (x, y, w, h): Rect) -> Result<Vec<u8>, String> {
        let b = Buffer::readback(&self.device, w as usize * h as usize * 4)?;
        let dst = b.raw;
        let result = self.execute(lease, None, Some(b), |d, cmd, src| unsafe {
            let copy = vk::BufferImageCopy::default()
                .image_subresource(layers())
                .image_offset(vk::Offset3D {
                    x: x as i32,
                    y: y as i32,
                    z: 0,
                })
                .image_extent(vk::Extent3D {
                    width: w,
                    height: h,
                    depth: 1,
                });
            d.cmd_copy_image_to_buffer(
                cmd,
                src,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                dst,
                &[copy],
            );
            let host = vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::HOST_READ);
            d.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::HOST,
                Default::default(),
                &[host],
                &[],
                &[],
            );
        })?;
        result.ok_or("missing readback allocation")?.rgba()
    }
    fn execute(
        &mut self,
        lease: &Arc<Lease>,
        signal: Option<vk::Semaphore>,
        buffer: Option<Buffer>,
        record: impl FnOnce(&ash::Device, vk::CommandBuffer, vk::Image),
    ) -> Result<Option<Buffer>, String> {
        let src = lease.vk_drawing()?;
        if self.pending.is_some() {
            return Err("Vulkan transfer still pending".into());
        }
        let d = self.device.raw();
        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        // SAFETY: only this owner calls into the queue/pool. WSI acquire already
        // queued its ALL_COMMANDS wait. The draw semaphore supplies availability
        // across queues. A failed submission retains all possibly referenced
        // storage in `pending` until successful retirement (or quarantine).
        unsafe {
            d.reset_command_pool(self.pool, Default::default())
                .map_err(error)?;
            d.reset_fences(&[self.fence]).map_err(error)?;
            d.begin_command_buffer(self.cmd, &begin).map_err(error)?;
            record(d, self.cmd, src.src.image);
            d.end_command_buffer(self.cmd).map_err(error)?;
            let wait = src.wait_once();
            let stages = [vk::PipelineStageFlags::ALL_COMMANDS];
            let cmds = [self.cmd];
            let info = vk::SubmitInfo::default()
                .command_buffers(&cmds)
                .wait_semaphores(wait.as_slice())
                .wait_dst_stage_mask(&stages[..usize::from(wait.is_some())])
                .signal_semaphores(signal.as_slice());
            self.pending = Some((lease.clone(), buffer));
            d.queue_submit(self.device.queue(), &[info], self.fence)
                .map_err(error)?;
            d.wait_for_fences(&[self.fence], true, 1_000_000_000)
                .map_err(error)?;
        }
        Ok(self.pending.take().unwrap().1)
    }
}
