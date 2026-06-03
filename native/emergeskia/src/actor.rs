use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use std::{
    io,
    marker::PhantomData,
    thread::{self, JoinHandle},
};

pub(crate) trait ActorSpec: Sized + Send + 'static {
    type Msg: Send + 'static;
    type Args: Send + 'static;

    const NAME: &'static str;

    fn run(rx: Receiver<Self::Msg>, shutdown_rx: Receiver<()>, args: Self::Args);

    fn spawn(mailbox_size: usize, args: Self::Args) -> io::Result<Actor<Self>> {
        Actor::spawn(mailbox_size, args)
    }
}

pub(crate) struct Actor<A: ActorSpec> {
    tx: Option<Sender<A::Msg>>,
    shutdown_tx: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
    _actor: PhantomData<A>,
}

impl<A: ActorSpec> Actor<A> {
    fn spawn(mailbox_size: usize, args: A::Args) -> io::Result<Self> {
        let (tx, rx) = bounded(mailbox_size);
        let (shutdown_tx, shutdown_rx) = bounded(1);

        let thread = thread::Builder::new()
            .name(A::NAME.to_string())
            .spawn(move || A::run(rx, shutdown_rx, args))?;

        Ok(Self {
            tx: Some(tx),
            shutdown_tx: Some(shutdown_tx),
            thread: Some(thread),
            _actor: PhantomData,
        })
    }

    pub(crate) fn tx(&self) -> Option<Sender<A::Msg>> {
        self.tx.clone()
    }

    pub(crate) fn send(&self, msg: A::Msg) -> bool {
        let Some(tx) = &self.tx else {
            return false;
        };

        match tx.try_send(msg) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub(crate) fn stop(&mut self) {
        self.tx.take();

        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.try_send(());
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl<A: ActorSpec> Drop for Actor<A> {
    fn drop(&mut self) {
        self.stop();
    }
}
