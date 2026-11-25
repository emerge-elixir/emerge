//! One graphics/present queue on the Wayland worker. Each image owns its
//! completion semaphore and its own WSI retirement fence, not a frame counter.
use super::{Owner, gpu::ProbeError, handles::*};
use crate::{
    session::DeviceReport,
    vk_window::{
        capabilities::DrmNodeId,
        device::{DeviceRequirements, VulkanDevice},
        ganesh::{GANESH_TARGET_IMAGE_USAGE, GaneshContext, TargetImageState, VulkanTargetSurface},
        instance::VulkanInstance,
        raw,
    },
};
use ash::{khr, vk};
use skia_safe::{
    Surface,
    gpu::{self, backend_semaphores, vk as skvk},
};
use smithay_client_toolkit::shell::{WaylandSurface, xdg::window::Window};
use std::sync::Arc;
use wayland_client::Connection;
#[path = "../../backend/wayland/vulkan/acquire.rs"]
mod acquire;
use acquire::AcquirePool;
const ACQUIRE_TIMEOUT_NS: u64 = 0;
fn error(e: vk::Result) -> String {
    format!("Wayland Vulkan: {e:?}")
}
struct Wsi {
    loader: khr::surface::Instance,
    handle: vk::SurfaceKHR,
    _win: Window,
    _conn: Connection,
    _instance: Arc<VulkanInstance>,
}
impl Drop for Wsi {
    fn drop(&mut self) {
        unsafe { self.loader.destroy_surface(self.handle, None) }
    }
}
struct Image {
    surface: Option<VulkanTargetSurface>,
    signal: vk::Semaphore,
    present: vk::Fence,
    sent: bool,
    device: Arc<VulkanDevice>,
}
impl Drop for Image {
    fn drop(&mut self) {
        self.surface.take();
        unsafe {
            self.device.raw().destroy_fence(self.present, None);
            self.device.raw().destroy_semaphore(self.signal, None);
        }
    }
}
struct Chain {
    loader: khr::swapchain::Device,
    handle: vk::SwapchainKHR,
    images: Vec<Image>,
    acquires: Option<AcquirePool>,
    size: (u32, u32),
    generation: u64,
}
impl Drop for Chain {
    fn drop(&mut self) {
        self.acquires.take();
        self.images.clear();
        unsafe { self.loader.destroy_swapchain(self.handle, None) }
    }
}
struct Graph {
    chain: Option<Chain>,
    context: GaneshContext,
    device: Arc<VulkanDevice>,
    wsi: Wsi,
}
impl Graph {
    fn allocate(&mut self, size: (u32, u32), generation: u64) -> Result<Chain, ProbeError> {
        let (device, s) = (&self.device, &self.wsi);
        let pd = device.physical_device();
        let caps = unsafe {
            s.loader
                .get_physical_device_surface_capabilities(pd, s.handle)
        }
        .map_err(error)?;
        let formats =
            unsafe { s.loader.get_physical_device_surface_formats(pd, s.handle) }.map_err(error)?;
        let format = vk::Format::B8G8R8A8_UNORM;
        let color = vk::ColorSpaceKHR::SRGB_NONLINEAR;
        if !formats.iter().any(|f| {
            f.color_space == color && (f.format == format || f.format == vk::Format::UNDEFINED)
        }) || !caps
            .supported_usage_flags
            .contains(GANESH_TARGET_IMAGE_USAGE)
        {
            return Err(ProbeError::Unavailable(
                "surface lacks Ganesh BGRA8/transfer support".into(),
            ));
        }
        let alpha = [
            vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
            vk::CompositeAlphaFlagsKHR::OPAQUE,
        ]
        .into_iter()
        .find(|&a| caps.supported_composite_alpha.contains(a))
        .ok_or_else(|| ProbeError::Unavailable("no compatible Vulkan composite alpha".into()))?;
        let extent = vk::Extent2D {
            width: size.0,
            height: size.1,
        };
        if (caps.current_extent.width != u32::MAX && caps.current_extent != extent)
            || size.0 < caps.min_image_extent.width
            || size.1 < caps.min_image_extent.height
            || size.0 > caps.max_image_extent.width
            || size.1 > caps.max_image_extent.height
        {
            return Err(ProbeError::Fatal(
                "configured extent not representable by Vulkan".into(),
            ));
        }
        let count = caps.min_image_count.saturating_add(1);
        let count = if caps.max_image_count == 0 {
            count
        } else {
            count.min(caps.max_image_count)
        };
        let old = self
            .chain
            .as_ref()
            .map_or(vk::SwapchainKHR::null(), |c| c.handle);
        let ci = vk::SwapchainCreateInfoKHR::default()
            .surface(s.handle)
            .min_image_count(count)
            .image_format(format)
            .image_color_space(color)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(GANESH_TARGET_IMAGE_USAGE)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(alpha)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true)
            .old_swapchain(old);
        let loader = khr::swapchain::Device::new(device.instance().raw(), device.raw());
        let handle = unsafe { loader.create_swapchain(&ci, None) }.map_err(error)?;
        let mut chain = Chain {
            loader,
            handle,
            images: Vec::new(),
            acquires: None,
            size,
            generation,
        };
        let raw_images = unsafe { chain.loader.get_swapchain_images(handle) }.map_err(error)?;
        chain.acquires =
            Some(AcquirePool::new(device.clone(), raw_images.len(), generation).map_err(error)?);
        for image in raw_images {
            let state = TargetImageState {
                layout: vk::ImageLayout::UNDEFINED,
                queue_family_index: device.queue_family_index(),
            };
            let surface = VulkanTargetSurface::new(&mut self.context, image, size, state)?;
            let signal = unsafe {
                device
                    .raw()
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
            }
            .map_err(error)?;
            let mut im = Image {
                surface: Some(surface),
                signal,
                present: vk::Fence::null(),
                sent: false,
                device: device.clone(),
            };
            im.present = unsafe {
                device
                    .raw()
                    .create_fence(&vk::FenceCreateInfo::default(), None)
            }
            .map_err(error)?;
            chain.images.push(im);
        }
        Ok(chain)
    }
    fn retire(&mut self) -> Result<(), String> {
        if let Some(c) = &mut self.chain {
            c.acquires
                .as_mut()
                .expect("initialized pool")
                .retire()
                .map_err(error)?;
            let fences = c
                .images
                .iter()
                .filter(|i| i.sent)
                .map(|i| i.present)
                .collect::<Vec<_>>();
            if !fences.is_empty() {
                unsafe {
                    self.device
                        .raw()
                        .wait_for_fences(&fences, true, 250_000_000)
                }
                .map_err(error)?;
            }
            // A command-recording or submission error may have left Ganesh
            // work buffered. Flush while borrowed image handles are still live.
            let ctx = self.context.direct_context_mut()?;
            if ctx.flush_and_submit().is_err() || ctx.abandoned() {
                return Err("unable to retire Vulkan drawing context".into());
            }
            self.device.wait_idle("retire Ganesh queue")?;
        }
        Ok(())
    }
}
pub(super) struct VkPresenter {
    graph: Option<Box<Graph>>,
    owner: Arc<Owner>,
    // Generation, image index, and whether to recreate after its presentation.
    acquired: Option<(u64, usize, bool)>,
    pub(super) recreate: bool,
}
impl VkPresenter {
    pub(super) fn new(
        conn: &Connection,
        win: &Window,
        size: (u32, u32),
        node: Option<DrmNodeId>,
        owner: &Arc<Owner>,
    ) -> Result<(Self, DeviceReport), ProbeError> {
        let display = raw_display_handle(conn)?;
        let mut exts = ash_window::enumerate_required_extensions(display)
            .map_err(error)?
            .to_vec();
        exts.extend([
            khr::get_surface_capabilities2::NAME.as_ptr(),
            ash::ext::surface_maintenance1::NAME.as_ptr(),
        ]);
        let instance = VulkanInstance::probe(&exts)?
            .ok_or_else(|| ProbeError::Unavailable("no compatible Vulkan WSI driver".into()))?;
        let loader = khr::surface::Instance::new(instance.entry(), instance.raw());
        let handle = unsafe {
            ash_window::create_surface(
                instance.entry(),
                instance.raw(),
                display,
                raw_window_handle(win.wl_surface())?,
                None,
            )
        }
        .map_err(error)?;
        let wsi = Wsi {
            handle,
            loader,
            _win: win.clone(),
            _conn: conn.clone(),
            _instance: instance.clone(),
        };
        let device = VulkanDevice::probe_surface(
            instance,
            DeviceRequirements {
                required_extensions: &[
                    khr::swapchain::NAME,
                    ash::ext::swapchain_maintenance1::NAME,
                ],
                require_timestamps: false,
                compositor_device: node,
            },
            |pd, q| {
                unsafe {
                    wsi.loader
                        .get_physical_device_surface_support(pd, q, handle)
                }
                .map_err(error)
            },
        )?
        .ok_or_else(|| {
            ProbeError::Unavailable(
                "no eligible hardware Vulkan device with present-fence support".into(),
            )
        })?;
        let info = device.report();
        let report = DeviceReport {
            physical_device_name: info.physical_device_name.clone(),
            driver_name: info.driver_name.clone(),
            driver_id: info.driver_id.clone(),
            software: info.software,
            drm_node: node.map(|n| {
                let field = if device.identity().render_node == Some(n) {
                    "render"
                } else {
                    "primary"
                };
                crate::session::DrmReport {
                    path: format!("/dev/char/{}:{}", n.major, n.minor),
                    match_field: field.into(),
                    major: n.major,
                    minor: n.minor,
                }
            }),
        };
        let context = GaneshContext::new(&device)?;
        let mut graph = Box::new(Graph {
            chain: None,
            context,
            device,
            wsi,
        });
        graph.chain = Some(graph.allocate(size, 1)?);
        Ok((
            Self {
                graph: Some(graph),
                owner: owner.clone(),
                acquired: None,
                recreate: false,
            },
            report,
        ))
    }
    fn graph(&mut self) -> &mut Graph {
        self.graph.as_deref_mut().expect("live Vulkan graph")
    }
    pub(super) fn acquire(&mut self, size: (u32, u32)) -> Result<bool, String> {
        let recreate = self.recreate;
        let g = self.graph();
        let c = g.chain.as_ref().expect("initialized swapchain");
        if c.size != size || recreate {
            let next = c
                .generation
                .checked_add(1)
                .ok_or("swapchain generation exhausted")?;
            g.retire()?;
            g.chain = Some(g.allocate(size, next).map_err(ProbeError::reason)?);
            self.acquired = None;
        }
        self.recreate = false;
        let held = self.acquired;
        let g = self.graph();
        let c = g.chain.as_mut().unwrap();
        let token = if let Some(t) = held {
            t
        } else {
            match c.acquires.as_mut().unwrap().acquire(c.handle, c.generation) {
                Ok(Some((idx, suboptimal))) => (c.generation, idx as usize, suboptimal),
                Ok(None) => return Ok(false),
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.recreate = true;
                    return Ok(false);
                }
                Err(e) => return Err(error(e)),
            }
        };
        if token.0 != c.generation {
            return Err("stale acquired image".into());
        }
        let i = c
            .images
            .get_mut(token.1)
            .ok_or("invalid acquired image index")?;
        // Reacquisition is not a CPU wait on the previous WSI fence. Retain
        // this token on NOT_READY; never acquire/consume its wait a second time.
        if i.sent {
            if !unsafe { g.device.raw().get_fence_status(i.present) }.map_err(error)? {
                self.acquired = Some(token);
                return Ok(false);
            }
            unsafe { g.device.raw().reset_fences(&[i.present]) }.map_err(error)?;
            i.sent = false;
        }
        self.acquired = Some(token);
        Ok(true)
    }
    pub(super) fn context(&mut self) -> Result<&mut gpu::DirectContext, String> {
        self.graph().context.direct_context_mut()
    }
    pub(super) fn surface(&mut self) -> &mut Surface {
        let (_, idx, _) = self.acquired.expect("acquired before record");
        self.graph().chain.as_mut().unwrap().images[idx]
            .surface
            .as_mut()
            .unwrap()
            .surface_mut()
    }
    pub(super) fn present(&mut self) -> Result<bool, String> {
        let (_, idx, acquire_suboptimal) =
            self.acquired.take().ok_or("no acquired Vulkan target")?;
        let g = self.graph();
        let c = g.chain.as_mut().unwrap();
        let i = &mut c.images[idx];
        let ctx = g.context.direct_context_mut()?;
        let state = skvk::mutable_texture_states::new_vulkan(
            skvk::ImageLayout::PRESENT_SRC_KHR,
            g.device.queue_family_index(),
        );
        let mut signal = unsafe { backend_semaphores::make_vk(raw::semaphore_to_skia(i.signal)) };
        let mut info = gpu::FlushInfo::default();
        unsafe { info.set_signal_semaphores(std::slice::from_mut(&mut signal)) };
        if ctx.flush_surface_with_texture_state(
            i.surface.as_mut().unwrap().surface_mut(),
            &info,
            Some(&state),
        ) != Ok(gpu::SemaphoresSubmitted::Yes)
            || !ctx.submit(gpu::SyncCpu::No)
        {
            return Err("Vulkan render-finished submission failed".into());
        }
        let waits = [i.signal];
        let chains = [c.handle];
        let indices = [idx as u32];
        let fences = [i.present];
        let mut fence_info = vk::SwapchainPresentFenceInfoEXT::default().fences(&fences);
        let present = vk::PresentInfoKHR::default()
            .wait_semaphores(&waits)
            .swapchains(&chains)
            .image_indices(&indices)
            .push_next(&mut fence_info);
        // Mark before calling WSI: an uncertain result is not permission to destroy.
        i.sent = true;
        match unsafe { c.loader.queue_present(g.device.queue(), &present) } {
            Ok(suboptimal) => {
                self.recreate = acquire_suboptimal || suboptimal;
                Ok(true)
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.recreate = true;
                Ok(false)
            }
            Err(e) => Err(error(e)),
        }
    }
}
impl Drop for VkPresenter {
    fn drop(&mut self) {
        if let Some(mut graph) = self.graph.take() {
            if let Err(e) = graph.retire() {
                self.owner
                    .lifecycle
                    .quarantine(format!("Wayland Vulkan resources quarantined: {e}"));
                Box::leak(graph);
            } else {
                graph.chain.take();
                graph.context.shutdown();
            }
        }
    }
}
