//! Ganesh draws on a graphics queue; a distinct transfer/present queue
//! resolves only the winner into FIFO WSI. Acquisition and WSI retirement
//! fences still belong to their original image/generation.
mod targets;
mod transfer;
use super::{Lease, Owner, Ping, gpu::ProbeError, handles::*, textures::Producer};
use crate::session::capture::Rect;
use crate::{
    session::DeviceReport,
    vk_window::{
        capabilities::DrmNodeId,
        device::{DeviceRequirements, VulkanDevice},
        ganesh::GaneshContext,
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
pub(in crate::session) use targets::Drawing;
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
    image: vk::Image,
    signal: vk::Semaphore,
    present: vk::Fence,
    sent: bool,
    device: Arc<VulkanDevice>,
}
impl Drop for Image {
    fn drop(&mut self) {
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
    transfer: transfer::Transfer,
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
            .contains(vk::ImageUsageFlags::TRANSFER_DST)
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
            .image_usage(vk::ImageUsageFlags::TRANSFER_DST)
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
            let signal = unsafe {
                device
                    .raw()
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
            }
            .map_err(error)?;
            let mut im = Image {
                image,
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
            self.transfer.retire()?;
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
        ping: &Ping,
    ) -> Result<(Self, Producer, DeviceReport), ProbeError> {
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
        let device = VulkanDevice::probe_split_surface(
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
                "no hardware Vulkan device with independent draw and transfer/present queues and present-fence support".into(),
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
        let draw = device.clone();
        let producer = Producer::start(owner.clone(), ping.clone(), move || {
            targets::DrawContext::new(draw)
        })?;
        let mut graph = Box::new(Graph {
            transfer: transfer::Transfer::new(&device)?,
            chain: None,
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
            producer,
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
    pub(super) fn read(&mut self, lease: &Arc<Lease>, rect: Rect) -> Result<Vec<u8>, String> {
        self.graph().transfer.read(lease, rect)
    }
    pub(super) fn present(&mut self, lease: &Arc<Lease>) -> Result<bool, String> {
        let (_, idx, acquire_suboptimal) =
            self.acquired.take().ok_or("no acquired Vulkan target")?;
        let g = self.graph();
        let c = g.chain.as_mut().unwrap();
        let i = &mut c.images[idx];
        g.transfer.copy(lease, i.image, i.signal)?;
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
            }
        }
    }
}
