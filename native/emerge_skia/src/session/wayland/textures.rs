//! The drawing thread owns its context, painter and four writable targets.
//! A completed lease keeps a slot immutable until the last reader finishes.
use super::*;
use crate::{
    session::{Bundle, Pixels, Signal},
    tree::GeometryPainter,
};
use crossbeam_channel::{Sender, bounded};
use skia_safe::{Color, Surface};
use std::{sync::atomic::AtomicBool, thread::JoinHandle};

pub(in crate::session) enum Image {
    #[cfg(feature = "opengl")]
    Gl(skia_safe::gpu::BackendTexture),
    #[cfg(feature = "vulkan")]
    Vk(vulkan::Drawing),
}
pub(in crate::session) struct Lease {
    pub(super) image: Image,
    pub size: (u32, u32),
    pin: Option<Arc<()>>,
    wake: Signal,
}
impl Drop for Lease {
    fn drop(&mut self) {
        drop(self.pin.take());
        self.wake.notify();
    }
}

pub(super) trait Renderer {
    type Storage;
    fn allocate(&mut self, size: (u32, u32)) -> Result<(Surface, Self::Storage), String>;
    /// Resolve and prove completion before exposing a readable handle.
    fn finish(&mut self, surface: &mut Surface, storage: &Self::Storage) -> Result<Image, String>;
}
struct Slot<S> {
    size: (u32, u32),
    surface: Surface,
    storage: S,
    pin: Arc<()>,
}
struct Bank<S>([Option<Slot<S>>; 4]);
impl<S> Default for Bank<S> {
    fn default() -> Self {
        Self(std::array::from_fn(|_| None))
    }
}
impl<S> Bank<S> {
    fn free(&self, size: (u32, u32)) -> Option<usize> {
        self.0
            .iter()
            .enumerate()
            .filter(|(_, s)| s.as_ref().is_none_or(|s| Arc::strong_count(&s.pin) == 1))
            .min_by_key(|(_, s)| s.as_ref().is_none_or(|s| s.size != size))
            .map(|(i, _)| i)
    }
    fn draw(
        &mut self,
        device: &mut impl Renderer<Storage = S>,
        painter: &mut GeometryPainter,
        i: usize,
        bundle: &Bundle,
        owner: &Owner,
    ) -> Result<Arc<Lease>, String> {
        let size = bundle.view.size;
        if self.0[i].as_ref().is_none_or(|s| s.size != size) {
            let (surface, storage) = device.allocate(size)?;
            self.0[i] = Some(Slot {
                size,
                surface,
                storage,
                pin: Arc::new(()),
            });
        }
        let slot = self.0[i].as_mut().unwrap();
        let s = &mut slot.surface;
        s.canvas().clear(Color::TRANSPARENT);
        painter.record(s.canvas(), &bundle.scene.drawing)?;
        #[cfg(feature = "stage-probes")]
        owner.probes.pause(
            crate::session::probes::Stage::Drawing,
            bundle.id.serial,
            bundle.revision,
            owner,
        );
        let image = device.finish(s, &slot.storage)?;
        Ok(Arc::new(Lease {
            image,
            size,
            pin: Some(slot.pin.clone()),
            wake: owner.render.clone(),
        }))
    }
}
pub(super) struct Producer {
    cancel: Arc<AtomicBool>,
    wake: Signal,
    retire: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}
impl Producer {
    pub(super) fn start<R: Renderer>(
        owner: Arc<Owner>,
        ping: Ping,
        init: impl FnOnce() -> Result<R, String> + Send + 'static,
    ) -> Result<Self, String> {
        let (ready, started) = bounded(1);
        let (retire, retired) = bounded::<()>(0);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let wake = owner.render.clone();
        let thread = std::thread::Builder::new()
            .name("emerge-device-draw".into())
            .spawn(move || {
                let mut device = match init() {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = ready.send(Err(e));
                        return;
                    }
                };
                let mut slots = Bank::default();
                let mut painter = GeometryPainter::default();
                if ready.send(Ok(())).is_err() {
                    return;
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<(), String> {
                        while !stop.load(Ordering::Acquire)
                            && !owner.lifecycle.stop.load(Ordering::Acquire)
                        {
                            let (next, t) = {
                                let mut p = lock(&owner.pipe)?;
                                match p.view.and_then(|v| slots.free(v.size)) {
                                    Some(i) => {
                                        let (f, t) = owner.draw(&mut p);
                                        (f.map(|f| (i, f)), t)
                                    }
                                    None => (None, None),
                                }
                            };
                            let Some((i, f)) = next else {
                                owner.render.wait(t);
                                continue;
                            };
                            let installed = lock(&owner.front)?.clone();
                            let reuse = installed.and_then(|i| match i.pixels {
                                Pixels::Gpu(target)
                                    if i.bundle.view.generation == f.id.generation
                                        && i.bundle.scene.same_drawing(&f.value.scene) =>
                                {
                                    Some(target)
                                }
                                _ => None,
                            });
                            let image = match reuse {
                                Some(t) => t,
                                None => {
                                    slots.draw(&mut device, &mut painter, i, &f.value, &owner)?
                                }
                            };
                            let completed = f.map(|bundle| Completed {
                                bundle,
                                target: Target::Gpu(image),
                            });
                            owner.completed(completed)?;
                            ping.ping();
                        }
                        Ok(())
                    },
                ))
                .unwrap_or_else(|_| Err("GPU draw worker panicked".into()));
                if let Err(e) = result {
                    owner.lifecycle.fail(e);
                    ping.ping();
                    owner.actor.notify();
                }
                // In particular GL surfaces must outlive use of the borrowed
                // texture in the shared presentation context, also during shutdown.
                let _ = retired.recv();
            })
            .map_err(err)?;
        let handle = Self {
            cancel,
            wake,
            retire: Some(retire),
            thread: Some(thread),
        };
        started.recv().map_err(err)??;
        Ok(handle)
    }
}
impl Drop for Producer {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.wake.notify();
        self.retire.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
