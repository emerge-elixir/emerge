//! One outstanding request, one result, one acknowledgement. Only the tree
//! actor issues tickets and binds completions. The loader never borrows a tree.
use super::*;
use crate::{
    raster_assets::{Loader, Pictures, Request, Source},
    tree::Ticket,
};
use std::thread::JoinHandle;

pub(super) struct Assets {
    pictures: Pictures,
    loader: Option<Loader>,
    worker: Option<Worker>,
    inflight: bool,
}
impl Assets {
    pub fn new(loader: Loader) -> Self {
        Self {
            pictures: loader.pictures.clone(),
            loader: Some(loader),
            worker: None,
            inflight: false,
        }
    }
    /// Return whether an accepted completion may change layout/paint.
    pub fn drive(&mut self, tree: &mut Tree, wake: &Signal) -> Result<bool, String> {
        let mut changed = false;
        if self.inflight
            && let Some(w) = &self.worker
        {
            match w.results.try_recv() {
                Ok((job, source)) => {
                    changed = tree.complete_image(&job, source);
                    w.ack
                        .try_send(changed)
                        .map_err(|_| "image service stopped")?;
                    self.inflight = false;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => (),
                Err(_) => return Err("image service stopped".into()),
            }
        }
        tree.bind_ready_pictures(&self.pictures);
        if !self.inflight
            && let Some(job) = tree.next_image()
        {
            if let Some(mut loader) = self.loader.take() {
                self.worker = Worker::start(wake.clone(), move |req, publish| {
                    loader.fulfill(req, publish);
                })
                .ok();
            }
            if let Some(w) = &self.worker {
                w.jobs
                    .try_send(job)
                    .map_err(|_| "image service unavailable")?;
                self.inflight = true;
            } else {
                changed |= tree.complete_image(&job.0, Source::Failed);
                wake.notify();
            }
        }
        Ok(changed)
    }
    pub fn stop(self) {
        if let Some(worker) = self.worker {
            worker.stop();
        }
    }
}
struct Worker {
    jobs: Sender<(Ticket, Request)>,
    results: Receiver<(Ticket, Source)>,
    ack: Sender<bool>,
    thread: JoinHandle<()>,
}
impl Worker {
    fn start(
        wake: Signal,
        mut load: impl FnMut(Request, &mut dyn FnMut(Source) -> bool) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (jobs, rx) = bounded::<(Ticket, Request)>(1);
        let (tx, results) = bounded(1);
        let (ack, acknowledged) = bounded(1);
        let thread = std::thread::Builder::new()
            .name("emerge-image".into())
            .spawn(move || {
                while let Ok((ticket, request)) = rx.recv() {
                    let mut ticket = Some(ticket);
                    load(request, &mut |source| {
                        let Some(ticket) = ticket.take() else {
                            return false;
                        };
                        if tx.try_send((ticket, source)).is_err() {
                            return false;
                        }
                        wake.notify();
                        acknowledged.recv().unwrap_or(false)
                    });
                }
            })?;
        Ok(Self {
            jobs,
            results,
            ack,
            thread,
        })
    }
    fn stop(self) {
        drop((self.jobs, self.results, self.ack));
        let _ = self.thread.join();
    }
}
#[cfg(test)]
mod tests;
