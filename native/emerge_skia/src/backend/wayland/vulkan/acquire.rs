//! WSI acquisition dependencies must cover Ganesh's initial layout transition, not just the
//! fragment/transfer stages guaranteed by SkSurface::wait. Queue a native ALL_COMMANDS wait on
//! Ganesh's queue before giving it the image. No CPU wait occurs during ordinary acquisition.

use std::{
    sync::Arc,
    thread::{self, ThreadId},
};

use ash::{khr, vk};

use crate::backend::vulkan::VulkanDevice;

use super::ACQUIRE_TIMEOUT_NS;

// One bounded teardown wait, not a per-slot deadline or a steady-state rendering wait.
const RETIRE_TIMEOUT_NS: u64 = 250_000_000;

// A narrow Vulkan-call seam: lifecycle tests execute the real pool without loading a GPU driver.
pub(super) trait AcquireApi: Clone {
    fn create_semaphore(&self) -> Result<vk::Semaphore, vk::Result>;
    fn create_fence(&self) -> Result<vk::Fence, vk::Result>;
    fn destroy_semaphore(&self, semaphore: vk::Semaphore);
    fn destroy_fence(&self, fence: vk::Fence);
    fn reset_fences(&self, fences: &[vk::Fence]) -> Result<(), vk::Result>;
    fn fence_ready(&self, fence: vk::Fence) -> Result<bool, vk::Result>;
    fn wait_fences(&self, fences: &[vk::Fence], timeout: u64) -> Result<(), vk::Result>;
    fn acquire(
        &self,
        swapchain: vk::SwapchainKHR,
        timeout: u64,
        semaphore: vk::Semaphore,
        fence: vk::Fence,
    ) -> Result<(u32, bool), vk::Result>;
    fn queue(&self) -> vk::Queue;
    fn submit(
        &self,
        queue: vk::Queue,
        info: &vk::SubmitInfo<'_>,
        fence: vk::Fence,
    ) -> Result<(), vk::Result>;
}

#[derive(Clone)]
pub(super) struct NativeApi {
    device: Arc<VulkanDevice>,
    loader: khr::swapchain::Device,
    owner: ThreadId,
}

impl AcquireApi for NativeApi {
    fn create_semaphore(&self) -> Result<vk::Semaphore, vk::Result> {
        unsafe {
            self.device
                .raw()
                .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
        }
    }
    fn create_fence(&self) -> Result<vk::Fence, vk::Result> {
        unsafe {
            self.device
                .raw()
                .create_fence(&vk::FenceCreateInfo::default(), None)
        }
    }
    fn destroy_semaphore(&self, semaphore: vk::Semaphore) {
        unsafe { self.device.raw().destroy_semaphore(semaphore, None) };
    }
    fn destroy_fence(&self, fence: vk::Fence) {
        unsafe { self.device.raw().destroy_fence(fence, None) };
    }
    fn reset_fences(&self, fences: &[vk::Fence]) -> Result<(), vk::Result> {
        unsafe { self.device.raw().reset_fences(fences) }
    }
    fn fence_ready(&self, fence: vk::Fence) -> Result<bool, vk::Result> {
        unsafe { self.device.raw().get_fence_status(fence) }
    }
    fn wait_fences(&self, fences: &[vk::Fence], timeout: u64) -> Result<(), vk::Result> {
        unsafe { self.device.raw().wait_for_fences(fences, true, timeout) }
    }
    fn acquire(
        &self,
        swapchain: vk::SwapchainKHR,
        timeout: u64,
        semaphore: vk::Semaphore,
        fence: vk::Fence,
    ) -> Result<(u32, bool), vk::Result> {
        unsafe {
            self.loader
                .acquire_next_image(swapchain, timeout, semaphore, fence)
        }
    }
    fn queue(&self) -> vk::Queue {
        self.device.queue()
    }
    fn submit(
        &self,
        queue: vk::Queue,
        info: &vk::SubmitInfo<'_>,
        fence: vk::Fence,
    ) -> Result<(), vk::Result> {
        if thread::current().id() != self.owner || queue != self.device.queue() {
            return Err(vk::Result::ERROR_VALIDATION_FAILED_EXT);
        }
        // SAFETY: the presenter owns queue host access. Ganesh uses this same queue on this thread.
        unsafe {
            self.device
                .raw()
                .queue_submit(queue, std::slice::from_ref(info), fence)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Available,
    Acquiring,
    Acquired(u32),
    Submitted(u32),
    Uncertain,
    Retired,
}

struct Slot<A: AcquireApi> {
    api: A,
    semaphore: vk::Semaphore,
    acquired: vk::Fence,
    submitted: vk::Fence,
    state: State,
}

impl<A: AcquireApi> Slot<A> {
    fn new(api: A) -> Result<Self, vk::Result> {
        // Start with null handles so partial allocation and partial pool construction unwind safely.
        let mut slot = Self {
            api,
            semaphore: vk::Semaphore::null(),
            acquired: vk::Fence::null(),
            submitted: vk::Fence::null(),
            state: State::Available,
        };
        slot.semaphore = slot.api.create_semaphore()?;
        slot.acquired = slot.api.create_fence()?;
        slot.submitted = slot.api.create_fence()?;
        Ok(slot)
    }

    fn poll(&mut self) -> Result<(), vk::Result> {
        if matches!(self.state, State::Submitted(_))
            && self.api.fence_ready(self.acquired)?
            && self.api.fence_ready(self.submitted)?
        {
            self.state = State::Available;
        }
        Ok(())
    }

    fn pending_fences(&self) -> impl Iterator<Item = vk::Fence> {
        [
            matches!(
                self.state,
                State::Acquiring | State::Acquired(_) | State::Submitted(_)
            )
            .then_some(self.acquired),
            matches!(self.state, State::Submitted(_)).then_some(self.submitted),
        ]
        .into_iter()
        .flatten()
    }
}

impl<A: AcquireApi> Drop for Slot<A> {
    fn drop(&mut self) {
        if !matches!(self.state, State::Available | State::Retired) {
            // Defensive unwind protection. The presenter also quarantines its entire native graph
            // on failed retirement. Keep the device alive if a pending slot is ever dropped alone.
            std::mem::forget(self.api.clone());
            return;
        }
        // SAFETY (native API): no WSI/queue operation references these objects; null handles are OK.
        self.api.destroy_fence(self.submitted);
        self.api.destroy_fence(self.acquired);
        self.api.destroy_semaphore(self.semaphore);
    }
}

pub(super) struct AcquirePool<A: AcquireApi = NativeApi> {
    slots: Vec<Slot<A>>,
    generation: u64,
    terminal: bool,
}

impl AcquirePool {
    pub(super) fn new(
        device: Arc<VulkanDevice>,
        image_count: usize,
        generation: u64,
    ) -> Result<Self, vk::Result> {
        let loader = khr::swapchain::Device::new(device.instance().raw(), device.raw());
        Self::with_api(
            NativeApi {
                device,
                loader,
                owner: thread::current().id(),
            },
            image_count,
            generation,
        )
    }
}

impl<A: AcquireApi> AcquirePool<A> {
    fn with_api(api: A, image_count: usize, generation: u64) -> Result<Self, vk::Result> {
        if image_count == 0 {
            return Err(vk::Result::ERROR_INITIALIZATION_FAILED);
        }
        Ok(Self {
            slots: (0..image_count)
                .map(|_| Slot::new(api.clone()))
                .collect::<Result<_, _>>()?,
            generation,
            terminal: false,
        })
    }

    /// Returns an image only after its native wait has been accepted. Errors after WSI success
    /// leave ownership recorded in the pool even if the caller never obtains an image token.
    pub(super) fn acquire(
        &mut self,
        swapchain: vk::SwapchainKHR,
        generation: u64,
    ) -> Result<Option<(u32, bool)>, vk::Result> {
        if self.terminal || generation != self.generation {
            return Err(vk::Result::ERROR_VALIDATION_FAILED_EXT);
        }
        let result = self.acquire_inner(swapchain);
        if let Err(error) = result {
            self.terminal = error != vk::Result::ERROR_OUT_OF_DATE_KHR;
        }
        result
    }

    fn acquire_inner(
        &mut self,
        swapchain: vk::SwapchainKHR,
    ) -> Result<Option<(u32, bool)>, vk::Result> {
        self.slots.iter_mut().try_for_each(Slot::poll)?;
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.state == State::Available)
        else {
            return Ok(None);
        };
        slot.api.reset_fences(&[slot.acquired, slot.submitted])?;
        slot.state = State::Acquiring;
        let image =
            match slot
                .api
                .acquire(swapchain, ACQUIRE_TIMEOUT_NS, slot.semaphore, slot.acquired)
            {
                Ok(image) => image,
                Err(vk::Result::ERROR_DEVICE_LOST) => {
                    slot.state = State::Uncertain;
                    return Err(vk::Result::ERROR_DEVICE_LOST);
                }
                Err(error) => {
                    // Unsuccessful WSI acquisition leaves semaphore/fence unaffected (except loss).
                    slot.state = State::Available;
                    return match error {
                        vk::Result::NOT_READY | vk::Result::TIMEOUT => Ok(None),
                        _ => Err(error),
                    };
                }
            };
        slot.state = State::Acquired(image.0);
        let waits = [slot.semaphore];
        let stages = [vk::PipelineStageFlags::ALL_COMMANDS];
        let submit = vk::SubmitInfo::default()
            .wait_semaphores(&waits)
            .wait_dst_stage_mask(&stages);
        // The wait's second scope includes later submissions on this queue. ALL_COMMANDS also
        // covers Ganesh's initial TOP_OF_PIPE layout transition; SkSurface::wait does not.
        if let Err(error) = slot.api.submit(slot.api.queue(), &submit, slot.submitted) {
            if error == vk::Result::ERROR_DEVICE_LOST {
                slot.state = State::Uncertain;
            }
            return Err(error);
        }
        slot.state = State::Submitted(image.0);
        Ok(Some(image))
    }

    /// Teardown/recreation only. Acquired-but-unsubmitted slots await WSI, never an unused submit
    /// fence. Any failure leaves the pool terminal and its unresolved objects quarantined.
    pub(super) fn retire(&mut self) -> Result<(), vk::Result> {
        self.terminal = true;
        if self.slots.iter().any(|slot| slot.state == State::Uncertain) {
            return Err(vk::Result::ERROR_DEVICE_LOST);
        }
        let fences = self
            .slots
            .iter()
            .flat_map(Slot::pending_fences)
            .collect::<Vec<_>>();
        if !fences.is_empty() {
            self.slots[0].api.wait_fences(&fences, RETIRE_TIMEOUT_NS)?;
        }
        self.slots
            .iter_mut()
            .for_each(|slot| slot.state = State::Retired);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
