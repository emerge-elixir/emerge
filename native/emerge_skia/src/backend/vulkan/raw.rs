//! Audited raw-handle boundary between `ash` and rust-skia Vulkan aliases.
//!
//! `ash` wraps every Vulkan handle in a Rust newtype, while rust-skia exposes the generated
//! Vulkan aliases used by Skia. They are ABI handles, but not interchangeable Rust types. All
//! integer/pointer casts live here so presenter and Ganesh code cannot grow ad-hoc conversions.

use std::{ffi::c_void, ptr};

use ash::{Entry, Instance, vk, vk::Handle as _};
use skia_safe::gpu::vk as sk_vk;

/// The Vulkan ABI represents dispatchable handles as pointers and non-dispatchable handles as
/// `u64` (or pointer-sized aliases in the generated rust-skia bindings). The intermediate `usize`
/// preserves the pointer representation used by rust-skia on supported 64-bit Linux targets.
macro_rules! ash_to_skia {
    ($name:ident, $ash:ty, $skia:ty) => {
        pub(crate) unsafe fn $name(handle: $ash) -> $skia {
            handle.as_raw() as usize as _
        }
    };
}

ash_to_skia!(instance_to_skia, vk::Instance, sk_vk::Instance);
ash_to_skia!(
    physical_device_to_skia,
    vk::PhysicalDevice,
    sk_vk::PhysicalDevice
);
ash_to_skia!(device_to_skia, vk::Device, sk_vk::Device);
ash_to_skia!(queue_to_skia, vk::Queue, sk_vk::Queue);
ash_to_skia!(image_to_skia, vk::Image, sk_vk::Image);
ash_to_skia!(semaphore_to_skia, vk::Semaphore, sk_vk::Semaphore);

pub(crate) fn image_layout_to_skia(layout: vk::ImageLayout) -> Result<sk_vk::ImageLayout, String> {
    match layout {
        vk::ImageLayout::UNDEFINED => Ok(sk_vk::ImageLayout::UNDEFINED),
        vk::ImageLayout::GENERAL => Ok(sk_vk::ImageLayout::GENERAL),
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL => {
            Ok(sk_vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        }
        vk::ImageLayout::TRANSFER_SRC_OPTIMAL => Ok(sk_vk::ImageLayout::TRANSFER_SRC_OPTIMAL),
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => Ok(sk_vk::ImageLayout::TRANSFER_DST_OPTIMAL),
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => {
            Ok(sk_vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        }
        vk::ImageLayout::PRESENT_SRC_KHR => Ok(sk_vk::ImageLayout::PRESENT_SRC_KHR),
        _ => Err(format!(
            "Vulkan image layout {} is not supported by the shared Ganesh frame contract",
            layout.as_raw()
        )),
    }
}

unsafe fn instance_from_skia(handle: sk_vk::Instance) -> vk::Instance {
    vk::Instance::from_raw(handle as usize as u64)
}

unsafe fn device_from_skia(handle: sk_vk::Device) -> vk::Device {
    vk::Device::from_raw(handle as usize as u64)
}

/// Resolves the procedure requested by Skia through the live ash entry/instance pair.
///
/// # Safety
/// The rust-skia handles and name pointer originate from the `BackendContext` being constructed
/// from `entry`/`instance`; both ash owners must outlive every resolver call.
pub(crate) unsafe fn resolve_proc(
    entry: &Entry,
    instance: &Instance,
    request: sk_vk::GetProcOf,
) -> *const c_void {
    let proc = match request {
        sk_vk::GetProcOf::Instance(raw, name) => unsafe {
            entry.get_instance_proc_addr(instance_from_skia(raw), name)
        },
        sk_vk::GetProcOf::Device(raw, name) => unsafe {
            instance.get_device_proc_addr(device_from_skia(raw), name)
        },
    };

    proc.map_or(ptr::null(), |function| {
        function as *const () as *const c_void
    })
}
