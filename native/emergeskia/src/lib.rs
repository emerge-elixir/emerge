use rustler::{Atom, NifResult, ResourceArc};
use std::sync::Mutex;

mod actor;
mod tree;

use crate::actor::ActorSpec;
use crate::tree::{Tree, TreeActor, TreeArgs};

mod atoms {
    rustler::atoms! {
        ok,
        stopped,
        lock_fail,
    }
}

#[derive(rustler::NifMap)]
struct StartOpts {
    title: String,
    width: u32,
    height: u32,
}

struct Runtime {
    actors: Mutex<Option<Actors>>,
}

struct Actors {
    tree: Tree,
}

#[rustler::resource_impl]
impl rustler::Resource for Runtime {}

impl Runtime {
    fn stop(&self) -> Atom {
        let actors = {
            let Ok(mut guard) = self.actors.try_lock() else {
                return atoms::lock_fail();
            };

            guard.take()
        };

        if actors.is_some() {
            drop(actors);
            atoms::ok()
        } else {
            atoms::stopped()
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let actors = match self.actors.get_mut() {
            Ok(actors) => actors.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };

        drop(actors);
    }
}

#[rustler::nif(schedule = "DirtyIo")]
fn start(opts: StartOpts) -> NifResult<ResourceArc<Runtime>> {
    let tree = TreeActor::spawn(
        512,
        TreeArgs {
            width: opts.width,
            height: opts.height,
        }
    )
    .map_err(|err| rustler::Error::Term(Box::new(format!("failed to start tree actor: {err}"))))?;

    Ok(ResourceArc::new(Runtime {
        actors: Mutex::new(Some(Actors { tree })),
    }))
}

#[rustler::nif(schedule = "DirtyIo")]
fn stop(runtime: ResourceArc<Runtime>) -> Atom {
    runtime.stop()
}

rustler::init!("Elixir.EmergeSkia.Native");
