use super::*;
use ash::vk::Handle;
use std::{
    cell::RefCell,
    collections::{HashSet, VecDeque},
    rc::Rc,
};

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Reset(Vec<u64>),
    Acquire(u64, u64, u64, u64),
    Submit {
        queue: u64,
        semaphore: u64,
        stage: u32,
        fence: u64,
        commands: u32,
        signals: u32,
    },
    Wait(Vec<u64>, u64),
}

#[derive(Default)]
struct Driver {
    allocations: u64,
    live: HashSet<u64>,
    ready: HashSet<vk::Fence>,
    calls: Vec<Call>,
    acquisitions: VecDeque<Result<(u32, bool), vk::Result>>,
    allocation_failure: Option<u64>,
    submit_error: Option<vk::Result>,
    wait_error: Option<vk::Result>,
    poll_error: Option<vk::Result>,
    reset_error: Option<vk::Result>,
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Driver>>);

impl Fake {
    fn allocate(&self) -> Result<u64, vk::Result> {
        let mut driver = self.0.borrow_mut();
        driver.allocations += 1;
        if driver.allocation_failure == Some(driver.allocations) {
            return Err(vk::Result::ERROR_OUT_OF_HOST_MEMORY);
        }
        let handle = driver.allocations;
        assert!(driver.live.insert(handle));
        Ok(handle)
    }
    fn destroy(&self, handle: u64) {
        if handle != 0 {
            assert!(
                self.0.borrow_mut().live.remove(&handle),
                "double destruction"
            );
        }
    }
    fn signal(&self, fences: &[vk::Fence]) {
        self.0.borrow_mut().ready.extend(fences.iter().copied());
    }
    fn pool(&self, count: usize) -> AcquirePool<Self> {
        AcquirePool::with_api(self.clone(), count, 7).unwrap()
    }
}

impl AcquireApi for Fake {
    fn create_semaphore(&self) -> Result<vk::Semaphore, vk::Result> {
        self.allocate().map(vk::Semaphore::from_raw)
    }
    fn create_fence(&self) -> Result<vk::Fence, vk::Result> {
        self.allocate().map(vk::Fence::from_raw)
    }
    fn destroy_semaphore(&self, semaphore: vk::Semaphore) {
        self.destroy(semaphore.as_raw());
    }
    fn destroy_fence(&self, fence: vk::Fence) {
        self.destroy(fence.as_raw());
    }
    fn reset_fences(&self, fences: &[vk::Fence]) -> Result<(), vk::Result> {
        let mut driver = self.0.borrow_mut();
        driver
            .calls
            .push(Call::Reset(fences.iter().map(|f| f.as_raw()).collect()));
        if let Some(error) = driver.reset_error {
            return Err(error);
        }
        driver.ready.retain(|fence| !fences.contains(fence));
        Ok(())
    }
    fn fence_ready(&self, fence: vk::Fence) -> Result<bool, vk::Result> {
        let driver = self.0.borrow();
        driver
            .poll_error
            .map_or(Ok(driver.ready.contains(&fence)), Err)
    }
    fn wait_fences(&self, fences: &[vk::Fence], timeout: u64) -> Result<(), vk::Result> {
        let mut driver = self.0.borrow_mut();
        driver.calls.push(Call::Wait(
            fences.iter().map(|f| f.as_raw()).collect(),
            timeout,
        ));
        driver.wait_error.map_or(Ok(()), Err)
    }
    fn acquire(
        &self,
        swapchain: vk::SwapchainKHR,
        timeout: u64,
        semaphore: vk::Semaphore,
        fence: vk::Fence,
    ) -> Result<(u32, bool), vk::Result> {
        let mut driver = self.0.borrow_mut();
        driver.calls.push(Call::Acquire(
            swapchain.as_raw(),
            timeout,
            semaphore.as_raw(),
            fence.as_raw(),
        ));
        driver.acquisitions.pop_front().unwrap_or(Ok((2, false)))
    }
    fn queue(&self) -> vk::Queue {
        vk::Queue::from_raw(53)
    }
    fn submit(
        &self,
        queue: vk::Queue,
        info: &vk::SubmitInfo<'_>,
        fence: vk::Fence,
    ) -> Result<(), vk::Result> {
        assert_eq!(info.wait_semaphore_count, 1);
        // Inspect the actual production SubmitInfo, not a parallel model of its contents.
        let (semaphore, stage) = unsafe { (*info.p_wait_semaphores, *info.p_wait_dst_stage_mask) };
        let mut driver = self.0.borrow_mut();
        driver.calls.push(Call::Submit {
            queue: queue.as_raw(),
            semaphore: semaphore.as_raw(),
            stage: stage.as_raw(),
            fence: fence.as_raw(),
            commands: info.command_buffer_count,
            signals: info.signal_semaphore_count,
        });
        driver.submit_error.map_or(Ok(()), Err)
    }
}

const SWAPCHAIN: vk::SwapchainKHR = vk::SwapchainKHR::null();

#[test]
fn wait_precedes_image_handoff_and_covers_later_ganesh_submissions() {
    let api = Fake::default();
    let mut pool = api.pool(3);
    api.0.borrow_mut().acquisitions.push_back(Ok((2, true)));
    assert_eq!(pool.acquire(SWAPCHAIN, 7), Ok(Some((2, true))));
    assert_eq!(pool.slots[0].state, State::Submitted(2)); // not acquisition-slot index zero
    assert_eq!(
        api.0.borrow().calls,
        [
            Call::Reset(vec![2, 3]),
            Call::Acquire(0, 0, 1, 2),
            Call::Submit {
                queue: 53,
                semaphore: 1,
                stage: vk::PipelineStageFlags::ALL_COMMANDS.as_raw(),
                fence: 3,
                commands: 0,
                signals: 0
            },
        ]
    );
    pool.retire().unwrap();
    drop(pool);
    assert!(api.0.borrow().live.is_empty());
}

#[test]
fn bounded_exhaustion_never_allocates_waits_or_calls_wsi_again() {
    let api = Fake::default();
    let mut pool = api.pool(3);
    for _ in 0..3 {
        assert!(pool.acquire(SWAPCHAIN, 7).unwrap().is_some());
    }
    let calls = api.0.borrow().calls.len();
    for _ in 0..1_000 {
        assert_eq!(pool.acquire(SWAPCHAIN, 7), Ok(None));
    }
    assert_eq!(api.0.borrow().allocations, 9);
    assert_eq!(api.0.borrow().calls.len(), calls);
    pool.retire().unwrap();
}

#[test]
fn reuse_requires_both_fences_in_either_observation_order() {
    for submission_first in [false, true] {
        let api = Fake::default();
        let mut pool = api.pool(1);
        pool.acquire(SWAPCHAIN, 7).unwrap();
        let slot = &pool.slots[0];
        let (first, second) = if submission_first {
            (slot.submitted, slot.acquired)
        } else {
            (slot.acquired, slot.submitted)
        };
        api.signal(&[first]);
        assert_eq!(pool.acquire(SWAPCHAIN, 7), Ok(None));
        api.signal(&[second]);
        assert!(pool.acquire(SWAPCHAIN, 7).unwrap().is_some());
        assert!(
            api.0.borrow().ready.is_empty(),
            "both fences reset before reuse"
        );
        assert_eq!(api.0.borrow().allocations, 3);
        pool.retire().unwrap();
    }
}

#[test]
fn completed_later_slot_does_not_wait_for_earlier_slot() {
    let api = Fake::default();
    let mut pool = api.pool(2);
    pool.acquire(SWAPCHAIN, 7).unwrap();
    pool.acquire(SWAPCHAIN, 7).unwrap();
    api.signal(&[pool.slots[1].acquired, pool.slots[1].submitted]);
    assert!(pool.acquire(SWAPCHAIN, 7).unwrap().is_some());
    assert_eq!(pool.slots[0].state, State::Submitted(2));
    assert!(matches!(
        api.0.borrow().calls.last(),
        Some(Call::Submit { semaphore: 4, .. })
    ));
    pool.retire().unwrap();
}

#[test]
fn unsuccessful_acquisition_can_retry_without_waiting_or_leaking() {
    for error in [
        vk::Result::NOT_READY,
        vk::Result::TIMEOUT,
        vk::Result::ERROR_OUT_OF_DATE_KHR,
    ] {
        let api = Fake::default();
        let mut pool = api.pool(1);
        api.0.borrow_mut().acquisitions.push_back(Err(error));
        let result = pool.acquire(SWAPCHAIN, 7);
        if error == vk::Result::ERROR_OUT_OF_DATE_KHR {
            assert_eq!(result, Err(error));
        } else {
            assert_eq!(result, Ok(None));
        }
        assert!(
            !api.0
                .borrow()
                .calls
                .iter()
                .any(|call| matches!(call, Call::Submit { .. } | Call::Wait(..)))
        );
        assert!(pool.acquire(SWAPCHAIN, 7).unwrap().is_some());
        pool.retire().unwrap();
        drop(pool);
        assert!(api.0.borrow().live.is_empty());
    }
}

#[test]
fn partial_preallocation_unwinds_every_created_object() {
    for failure in 1..=9 {
        let api = Fake::default();
        api.0.borrow_mut().allocation_failure = Some(failure);
        assert!(AcquirePool::with_api(api.clone(), 3, 7).is_err());
        assert!(api.0.borrow().live.is_empty(), "failure at {failure}");
        assert!(api.0.borrow().calls.is_empty());
    }
}

#[test]
fn failed_submit_is_terminal_and_retires_only_the_wsi_fence() {
    let api = Fake::default();
    let mut pool = api.pool(1);
    api.0.borrow_mut().submit_error = Some(vk::Result::ERROR_OUT_OF_HOST_MEMORY);
    assert_eq!(
        pool.acquire(SWAPCHAIN, 7),
        Err(vk::Result::ERROR_OUT_OF_HOST_MEMORY)
    );
    assert_eq!(pool.slots[0].state, State::Acquired(2));
    assert!(pool.acquire(SWAPCHAIN, 7).is_err());
    assert_eq!(api.0.borrow().live.len(), 3);
    pool.retire().unwrap();
    assert_eq!(
        api.0.borrow().calls.last(),
        Some(&Call::Wait(vec![2], RETIRE_TIMEOUT_NS))
    );
    drop(pool);
    assert!(api.0.borrow().live.is_empty());
}

#[test]
fn caller_failure_after_handoff_keeps_the_submitted_wait_owned() {
    let api = Fake::default();
    let mut pool = api.pool(1);
    // Simulate rejected target/index, draw failure or capture failure: no successful present.
    let _discarded_image = pool.acquire(SWAPCHAIN, 7).unwrap();
    assert_eq!(api.0.borrow().live.len(), 3);
    pool.retire().unwrap();
    assert_eq!(
        api.0.borrow().calls.last(),
        Some(&Call::Wait(vec![2, 3], RETIRE_TIMEOUT_NS))
    );
    assert_eq!(
        api.0
            .borrow()
            .calls
            .iter()
            .filter(|call| matches!(call, Call::Submit { .. }))
            .count(),
        1
    );
    drop(pool);
    assert!(api.0.borrow().live.is_empty());
}

#[test]
fn stale_generation_and_retired_pool_cannot_touch_wsi() {
    let api = Fake::default();
    let mut pool = api.pool(1);
    assert!(pool.acquire(SWAPCHAIN, 6).is_err());
    assert!(api.0.borrow().calls.is_empty());
    pool.retire().unwrap();
    assert!(pool.acquire(SWAPCHAIN, 7).is_err());
    assert!(api.0.borrow().calls.is_empty());
    drop(pool);
    assert!(api.0.borrow().live.is_empty());
}

#[test]
fn reset_and_poll_errors_do_not_admit_more_work() {
    let api = Fake::default();
    let mut pool = api.pool(1);
    api.0.borrow_mut().reset_error = Some(vk::Result::ERROR_OUT_OF_HOST_MEMORY);
    assert!(pool.acquire(SWAPCHAIN, 7).is_err());
    assert!(
        !api.0
            .borrow()
            .calls
            .iter()
            .any(|call| matches!(call, Call::Acquire(..)))
    );
    pool.retire().unwrap();
    drop(pool);
    assert!(api.0.borrow().live.is_empty());

    let api = Fake::default();
    let mut pool = api.pool(1);
    pool.acquire(SWAPCHAIN, 7).unwrap();
    api.0.borrow_mut().poll_error = Some(vk::Result::ERROR_OUT_OF_HOST_MEMORY);
    assert!(pool.acquire(SWAPCHAIN, 7).is_err());
    assert!(pool.terminal);
    pool.retire().unwrap();
}

#[test]
fn unresolved_retirement_quarantines_sync_objects_and_the_device_owner() {
    let api = Fake::default();
    let mut pool = api.pool(1);
    pool.acquire(SWAPCHAIN, 7).unwrap();
    api.0.borrow_mut().wait_error = Some(vk::Result::TIMEOUT);
    assert_eq!(pool.retire(), Err(vk::Result::TIMEOUT));
    assert!(pool.acquire(SWAPCHAIN, 7).is_err());
    drop(pool);
    assert_eq!(api.0.borrow().live.len(), 3);
    assert_eq!(
        Rc::strong_count(&api.0),
        2,
        "quarantine retains its device owner"
    );
}

#[test]
fn device_loss_during_acquire_or_submit_never_recycles_uncertain_objects() {
    for submit in [false, true] {
        let api = Fake::default();
        let mut pool = api.pool(1);
        if submit {
            api.0.borrow_mut().submit_error = Some(vk::Result::ERROR_DEVICE_LOST);
        } else {
            api.0
                .borrow_mut()
                .acquisitions
                .push_back(Err(vk::Result::ERROR_DEVICE_LOST));
        }
        assert_eq!(
            pool.acquire(SWAPCHAIN, 7),
            Err(vk::Result::ERROR_DEVICE_LOST)
        );
        assert_eq!(pool.retire(), Err(vk::Result::ERROR_DEVICE_LOST));
        assert!(
            !api.0
                .borrow()
                .calls
                .iter()
                .any(|call| matches!(call, Call::Wait(..)))
        );
        drop(pool);
        assert_eq!(api.0.borrow().live.len(), 3);
    }
}
