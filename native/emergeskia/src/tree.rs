use crate::actor::{Actor, ActorSpec};
use crossbeam_channel::{Receiver, select};

mod element;

pub(crate) type Tree = Actor<TreeActor>;

pub(crate) struct TreeActor;

pub(crate) struct TreeArgs {
    pub width: u32,
    pub height: u32,
}

pub(crate) enum TreeMsg {
    Render(Vec<u8>),
    Patch(Vec<u8>),
}

struct TreeState {
    width: u32,
    height: u32,
    generation: u64,
}

impl TreeState {
    fn new(args: TreeArgs) -> Self {
        Self {
            width: args.width,
            height: args.height,
            generation: 0,
        }
    }

    fn render(&mut self, bytes: Vec<u8>) {
        self.generation += 1;
    }
}

impl ActorSpec for TreeActor {
    type Msg = TreeMsg;
    type Args = TreeArgs;

    const NAME: &'static str = "emerge-tree";

    fn run(rx: Receiver<Self::Msg>, shutdown_rx: Receiver<()>, args: Self::Args) {
        let mut state = TreeState::new(args);

        loop {
            select! {
                recv(shutdown_rx) -> _ => break,

                recv(rx) -> msg => {
                    match msg {
                        Ok(TreeMsg::Render(bytes)) => state.render(bytes),
                        Ok(TreeMsg::Patch(bytes)) => break,
                        Err(_) => break,
                    }
                }
            }
        }
    }
}
