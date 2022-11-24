use crate::ChannelError;
use crate::{Mailbox, Message, Recipient};
use async_trait::async_trait;

/// Enable a struct to be used as an actor.
///
///
#[async_trait]
pub trait Actor: 'static + Sized + Send + Sync {
    /// Type of input messages this actor consumes
    type Input: Message;

    /// Type of output messages this actor produces
    type Output: Message;

    /// Type of the peers that actor is connected to
    type Peers: From<Recipient<Self::Output>>;

    /// Run the actor
    ///
    /// Processing input messages,
    /// updating internal state,
    /// and sending messages to peers.
    async fn run(
        self,
        messages: Mailbox<Self::Input>,
        peers: Self::Peers,
    ) -> Result<(), ChannelError>;
}

#[cfg(test)]
mod tests {
    use crate::test_utils::VecRecipient;
    use crate::*;
    use async_trait::async_trait;
    use tokio::spawn;

    struct Echo;

    #[async_trait]
    impl Actor for Echo {
        type Input = String;
        type Output = String;
        type Peers = Recipient<String>;

        async fn run(
            mut self,
            mut messages: Mailbox<Self::Input>,
            mut peers: Self::Peers,
        ) -> Result<(), ChannelError> {
            while let Some(message) = messages.next().await {
                peers.send(message).await?
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn running_an_actor_without_a_runtime() {
        let messages: VecRecipient<String> = VecRecipient::default();

        let actor = Echo;
        let (mailbox, input) = new_mailbox(10);
        let output = messages.as_recipient();

        let actor_task = spawn(actor.run(mailbox, output));

        spawn(async move {
            let mut input = input.as_recipient();
            input
                .send("Hello")
                .await
                .expect("the actor is still running");
            input
                .send("World")
                .await
                .expect("the actor is still running");
        });

        actor_task
            .await
            .expect("the actor run to completion")
            .expect("the actor returned Ok");

        assert_eq!(
            messages.collect().await,
            vec!["Hello".to_string(), "World".to_string()]
        )
    }
}
