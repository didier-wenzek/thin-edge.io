use crate::{Actor, Recipient, RunActor, RuntimeError, Task};
use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use tokio::task::JoinHandle;

/// Actions sent by actors to the runtime
#[derive(Debug)]
pub enum RuntimeAction {
    Shutdown,
    Spawn(Box<dyn Task>),
}

/// Requests sent by the runtime to actors
#[derive(Debug)]
pub enum RuntimeRequest {
    Shutdown,
}

/// Events published by the runtime
#[derive(Clone, Debug)]
pub enum RuntimeEvent {
    Error(RuntimeError),
    Started { task: String },
    Stopped { task: String },
    Aborted { task: String, error: RuntimeError },
}

/// The actor runtime
pub struct Runtime {
    handle: RuntimeHandle,
    bg_task: JoinHandle<()>,
}

impl Runtime {
    /// Launch the runtime returning a runtime handler
    ///
    /// TODO ensure this can only be called once
    pub async fn try_new(
        events_sender: Option<Recipient<RuntimeEvent>>,
    ) -> Result<Runtime, RuntimeError> {
        let (actions_sender, actions_receiver) = mpsc::channel(16);
        let runtime_actor = RuntimeActor {
            actions: actions_receiver,
            events: events_sender,
        };
        let runtime_task = tokio::spawn(runtime_actor.run());
        let runtime = Runtime {
            handle: RuntimeHandle { actions_sender },
            bg_task: runtime_task,
        };
        Ok(runtime)
    }

    pub fn get_handle(&self) -> RuntimeHandle {
        self.handle.clone()
    }

    /// Run the runtime up to completion
    ///
    /// I.e until
    /// - Either, a `Shutdown` action is sent to the runtime
    /// - Or, all the runtime handler clones have been dropped
    ///       and all the running tasks have reach completion (successfully or not).
    pub async fn run_to_completion(self) -> Result<(), RuntimeError> {
        let bg_task = self.drop_runtime_handle();
        Runtime::wait_for_completion(bg_task).await
    }

    /// Drop the runtime handle,
    ///
    /// Tell the runtime that no more actions will be sent from this handle
    /// and that new tasks and actors can only be created by already launched actors.
    fn drop_runtime_handle(self) -> JoinHandle<()> {
        self.bg_task
    }

    async fn wait_for_completion(bg_task: JoinHandle<()>) -> Result<(), RuntimeError> {
        bg_task.await.map_err(|err| {
            if err.is_panic() {
                RuntimeError::RuntimePanic
            } else {
                RuntimeError::RuntimeCancellation
            }
        })
    }
}

/// A handle passed to actors to interact with the runtime
#[derive(Clone)]
pub struct RuntimeHandle {
    actions_sender: mpsc::Sender<RuntimeAction>,
}

impl RuntimeHandle {
    /// Stop all the actors and the runtime
    pub async fn shutdown(&mut self) -> Result<(), RuntimeError> {
        self.send(RuntimeAction::Shutdown).await
    }

    /// Launch a task in the background
    pub async fn spawn(&mut self, task: impl Task) -> Result<(), RuntimeError> {
        self.send(RuntimeAction::Spawn(Box::new(task))).await
    }

    /// Launch an actor instance
    pub async fn run<A: Actor>(
        &mut self,
        actor: A,
        mailbox: A::Mailbox,
        peers: A::Peers,
    ) -> Result<(), RuntimeError> {
        self.spawn(RunActor::new(actor, mailbox, peers)).await
    }

    /// Send an action to the runtime
    pub async fn send(&mut self, action: RuntimeAction) -> Result<(), RuntimeError> {
        self.actions_sender.send(action).await?;
        Ok(())
    }
}

/// The actual runtime implementation
struct RuntimeActor {
    actions: mpsc::Receiver<RuntimeAction>,
    events: Option<Recipient<RuntimeEvent>>,
    // TODO store a join handle for each running task/actor
    // TODO store a sender of RuntimeRequest to each actors
}

impl RuntimeActor {
    async fn run(mut self) {
        // TODO select next action or next task completion
        while let Some(action) = self.actions.next().await {
            match action {
                RuntimeAction::Shutdown => {
                    todo!();
                    // TODO send a Shutdown request to each active actor
                    // TODO wait say 60 s, then cancel all tasks still running
                }
                RuntimeAction::Spawn(task) => {
                    tokio::spawn(task.run());

                    // TODO log a start event
                    // TODO log the end event on success and failure
                    // TODO store a recipient to send messages to the task/actor
                    // TODO store the join_handle : to be able to cancel the task
                }
            }
        }
    }
}
