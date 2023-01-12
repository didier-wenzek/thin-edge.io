use crate::ChannelError;
use crate::DynSender;
use crate::Message;
use crate::NullSender;
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::StreamExt;
use log::debug;
use log::info;

/// A message box used by an actor to collect all its input and forward its output
///
/// This message box can be seen as two streams of messages,
/// - inputs sent to the actor and stored in the message box awaiting to be processed,
/// - outputs sent by the actor and forwarded to the message boxes of other actors.
///
/// ```logical-view
///                                      +------+
/// input_sender: DynSender<Input> ----->| Box  |----> output_sender: DynSender<Input>
///                                      +------+
/// ```
///
/// Under the hood, a `MessageBox` implementation can use
/// - several input channels to await messages from specific peers
///   .e.g. awaiting a response from an HTTP actor
///    and ignoring the kind of events till a response or a timeout has been received.
/// - several output channels to send messages to specific peers.
/// - provide helper function that combine internal channels.
#[async_trait]
pub trait MessageBox: 'static + Sized + Send + Sync {
    /// Type of input messages the actor consumes
    type Input: Message;

    /// Type of output messages the actor produces
    type Output: Message;

    /// Return the next available input message if any
    ///
    /// Await for a message if there is not message yet.
    /// Return `None` if no more message can be received because all the senders have been dropped.
    async fn recv(&mut self) -> Option<Self::Input>;

    /// Send an output message.
    ///
    /// Fail if there is no more receiver expecting these messages.
    async fn send(&mut self, message: Self::Output) -> Result<(), ChannelError>;

    /// Turn on/off logging of input and output messages
    fn turn_logging_on(&mut self, on: bool);

    /// Name of the associated actor
    fn name(&self) -> &str;

    /// Log an input message just after reception, before processing it.
    fn log_input(&self, message: &Self::Input) {
        if self.logging_is_on() {
            info!(target: self.name(), "recv {:?}", message);
        }
    }

    /// Log an output message just before sending it.
    fn log_output(&self, message: &Self::Output) {
        if self.logging_is_on() {
            debug!(target: self.name(), "send {:?}", message);
        }
    }

    fn logging_is_on(&self) -> bool;
}

/// The basic message box
pub struct SimpleMessageBox<Input, Output> {
    name: String,
    input_receiver: mpsc::Receiver<Input>,
    output_sender: DynSender<Output>,
    logging_is_on: bool,
}

impl<Input: Message, Output: Message> SimpleMessageBox<Input, Output> {
    pub fn new(
        name: String,
        input_receiver: mpsc::Receiver<Input>,
        output_sender: DynSender<Output>,
    ) -> Self {
        SimpleMessageBox {
            name,
            input_receiver,
            output_sender,
            logging_is_on: true,
        }
    }

    /// Create a simple message box along an associated message box
    ///
    /// Messages sent from the associated message box are received by the main message box.
    /// Messages sent by the main message box are received from the associated message box.
    ///
    /// TODO Can this method replace MessageBox::new_box that happens to be difficult to impl and use?
    pub fn new_channel(name: &str) -> (SimpleMessageBox<Output, Input>, Self) {
        let (input_sender, input_receiver) = mpsc::channel(16);
        let (output_sender, output_receiver) = mpsc::channel(16);
        let main_box =
            SimpleMessageBox::new(name.to_string(), input_receiver, output_sender.into());
        let associated_box = SimpleMessageBox::new(
            format!("{}-Client", name),
            output_receiver,
            input_sender.into(),
        );
        (associated_box, main_box)
    }

    /// Close the sending channel of this message box.
    ///
    /// This makes the receiving end aware that no more message will be sent.
    pub fn close_output(&mut self) {
        self.output_sender = NullSender.into()
    }
}

#[async_trait]
impl<Input: Message, Output: Message> MessageBox for SimpleMessageBox<Input, Output> {
    type Input = Input;
    type Output = Output;

    async fn recv(&mut self) -> Option<Input> {
        self.input_receiver.next().await.map(|message| {
            self.log_input(&message);
            message
        })
    }

    async fn send(&mut self, message: Output) -> Result<(), ChannelError> {
        self.log_output(&message);
        self.output_sender.send(message).await
    }

    fn turn_logging_on(&mut self, on: bool) {
        self.logging_is_on = on;
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn logging_is_on(&self) -> bool {
        self.logging_is_on
    }
}

/// A message box for a request-response service
pub type ServiceMessageBox<Request, Response> =
    SimpleMessageBox<(ClientId, Request), (ClientId, Response)>;

pub type ClientId = usize;

/// A message box for services that handles requests concurrently
pub struct ConcurrentServiceMessageBox<Request, Response> {
    /// Max concurrent requests
    max_concurrency: usize,

    /// Message box to interact with clients of this service
    clients: ServiceMessageBox<Request, Response>,

    /// Pending responses
    pending_responses: futures::stream::FuturesUnordered<PendingResult<(usize, Response)>>,
}

type PendingResult<R> = tokio::task::JoinHandle<R>;

type RawClientMessageBox<Request, Response> =
    SimpleMessageBox<(ClientId, Response), (ClientId, Request)>;

impl<Request: Message, Response: Message> ConcurrentServiceMessageBox<Request, Response> {
    pub(crate) fn new(
        max_concurrency: usize,
        clients: ServiceMessageBox<Request, Response>,
    ) -> Self {
        ConcurrentServiceMessageBox {
            max_concurrency,
            clients,
            pending_responses: futures::stream::FuturesUnordered::new(),
        }
    }

    /// Create a service message box alongside an associated box for a test client
    ///
    /// In practice the associated box will be used only for tests,
    /// because all the requests and responses are multiplexed.
    pub fn new_channel(
        name: &str,
        max_concurrency: usize,
    ) -> (RawClientMessageBox<Request, Response>, Self) {
        let (clients_box, service_box) = SimpleMessageBox::new_channel(name);
        let concurrent_service_box = ConcurrentServiceMessageBox::new(max_concurrency, service_box);
        (clients_box, concurrent_service_box)
    }

    async fn next_request(&mut self) -> Option<(usize, Request)> {
        self.await_idle_processor().await;
        loop {
            tokio::select! {
                Some(request) = self.clients.recv() => {
                    return Some(request);
                }
                Some(result) = self.pending_responses.next() => {
                    self.send_result(result).await;
                }
                else => {
                    return None
                }
            }
        }
    }

    async fn await_idle_processor(&mut self) {
        if self.pending_responses.len() >= self.max_concurrency {
            if let Some(result) = self.pending_responses.next().await {
                self.send_result(result).await;
            }
        }
    }

    pub fn send_response_once_done(&mut self, pending_result: PendingResult<(ClientId, Response)>) {
        self.pending_responses.push(pending_result);
    }

    async fn send_result(&mut self, result: Result<(usize, Response), tokio::task::JoinError>) {
        if let Ok(response) = result {
            let _ = self.clients.send(response).await;
        }
        // TODO handle error cases:
        // - cancelled task
        // - task panics
        // - send fails
    }
}

#[async_trait]
impl<Request: Message, Response: Message> MessageBox
    for ConcurrentServiceMessageBox<Request, Response>
{
    type Input = (ClientId, Request);
    type Output = (ClientId, Response);

    async fn recv(&mut self) -> Option<Self::Input> {
        self.next_request().await
    }

    async fn send(&mut self, message: Self::Output) -> Result<(), ChannelError> {
        self.clients.send(message).await
    }

    fn turn_logging_on(&mut self, on: bool) {
        self.clients.turn_logging_on(on)
    }

    fn name(&self) -> &str {
        self.clients.name()
    }

    fn logging_is_on(&self) -> bool {
        self.clients.logging_is_on()
    }
}
