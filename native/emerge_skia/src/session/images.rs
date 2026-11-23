//! One bounded work signal, and no loader thread for a renderer without images.
//! Tickets live in the tree. Filesystem and codec calls never hold its lock.
use super::*;
use crate::raster_assets::{Loader, Request, Source};
use std::{sync::Weak, thread::JoinHandle};

pub(super) struct Assets {
    loader: Option<Loader>,
    worker: Option<Worker>,
}
impl Assets {
    pub fn new(loader: Loader) -> Self {
        Self {
            loader: Some(loader),
            worker: None,
        }
    }
    pub fn request(&mut self, owner: &Arc<Owner>, wake: &Wake, tree: &mut Tree) {
        if !tree.needs_images() {
            return;
        }
        if let Some(mut loader) = self.loader.take() {
            self.worker = Worker::start(
                Arc::downgrade(owner),
                wake.clone(),
                move |request, publish| {
                    loader.fulfill(request, publish);
                },
            )
            .ok();
        }
        if let Some(worker) = &self.worker {
            let _ = worker.notify.try_send(());
        } else {
            // The declaration is accepted. Failure to start the file service
            // is a failed resource, not a retroactive tree rejection.
            while let Some((job, _)) = tree.next_image() {
                tree.complete_image(&job, Source::Failed);
            }
        }
    }
    pub fn notify_fits(&self) {
        if let Some(w) = &self.worker {
            let _ = w.notify.try_send(());
        }
    }
    pub fn stop(self) {
        if let Some(w) = self.worker {
            w.stop();
        }
    }
}
struct Worker {
    notify: Sender<()>,
    thread: JoinHandle<()>,
}
impl Worker {
    fn start(
        owner: Weak<Owner>,
        wake: Wake,
        mut load: impl FnMut(Request, &mut dyn FnMut(Source) -> bool) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (notify, rx) = bounded(1);
        let thread = std::thread::Builder::new()
            .name("emerge-image".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    let Some(owner) = owner.upgrade() else { break };
                    loop {
                        if !owner.lifecycle.running.load(Ordering::Acquire) {
                            return;
                        }
                        let job = owner
                            .live
                            .lock()
                            .ok()
                            .and_then(|mut guard| guard.as_mut()?.raster.tree.next_image());
                        let Some((ticket, request)) = job else { break };
                        load(request, &mut |source| {
                            let Ok(mut guard) = owner.live.lock() else {
                                return false;
                            };
                            let Some(live) = guard.as_mut() else {
                                return false;
                            };
                            if !owner.lifecycle.running.load(Ordering::Acquire)
                                || !live.raster.tree.complete_image(&ticket, source)
                            {
                                return false;
                            }
                            // Partial fitted inputs cannot produce a frame. Let
                            // this worker finish the batch before waking display.
                            if !live.raster.tree.waiting_for_pixels() {
                                live.request = true;
                                wake.notify();
                            }
                            true
                        });
                    }
                }
            })?;
        Ok(Self { notify, thread })
    }
    fn stop(self) {
        drop(self.notify);
        let _ = self.thread.join();
    }
}
#[cfg(test)]
mod tests;
