//! Communication channels.

use crate::messages;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use std::collections::HashMap;
use thiserror::Error;

/// A message with a value of `T`, which expected to be returned back with a
/// value of `U`.
pub struct Callback<T, U> {
    /// Message sent in the forward direction.
    pub message: T,
    /// Channel for the return message.
    pub callback: oneshot::Sender<U>,
}

/// Node request with a response callback.
pub type NodeCallback = Callback<messages::NodeRequest, messages::NodeResponse>;

/// Set of incoming communication channels for a [`Carrier`](crate::Carrier).
pub struct Incoming {
    channels: HashMap<String, mpsc::Receiver<NodeCallback>>,
}

/// Set of outgoing communication channels for a [`Carrier`](crate::Carrier).
pub struct Outgoing {
    channels: HashMap<String, mpsc::Sender<NodeCallback>>,
}

/// Error returned by [`Callback::send`].
#[derive(Error, Debug)]
pub enum SendError {
    /// Forward channel closed.
    #[error("forward channel closed")]
    ForwardClosed(#[from] mpsc::SendError),
    /// Return channel closed.
    #[error("return channel closed")]
    ReturnClosed(#[from] oneshot::Canceled),
}

impl Incoming {
    pub(crate) fn new(channels: HashMap<String, mpsc::Receiver<NodeCallback>>) -> Self {
        Self { channels }
    }

    /// Receives the next request message from one of the nodes. The response is
    /// in the form `(node, callback)`. The response should be send back via the
    /// callback channel.
    pub async fn recv(&mut self) -> Option<(&str, NodeCallback)> {
        let (callback, _, _) = future::select_all(self.channels.iter_mut().map(|(node, rx)| {
            rx.next()
                .map(|callback| callback.map(|callback| (node.as_str(), callback)))
        }))
        .await;
        callback
    }
}

impl Outgoing {
    pub(crate) fn new(channels: HashMap<String, mpsc::Sender<NodeCallback>>) -> Self {
        Self { channels }
    }

    /// Sends a request `message` to `node` and awaits for the response.
    ///
    /// # Panics
    ///
    /// If `node` was not configured in [`Carrier::new`](crate::Carrier::new).
    pub async fn send(
        &mut self,
        node: &str,
        message: messages::NodeRequest,
    ) -> Result<messages::NodeResponse, SendError> {
        let (message, rx) = Callback::new(message);
        self.channels
            .get_mut(node)
            .expect("to be configured")
            .send(message)
            .await?;
        Ok(rx.await?)
    }
}

impl<T, U> Callback<T, U> {
    /// Creates a pair of a new [`Callback`] message and a corresponding callback
    /// from `message`.
    pub fn new(message: T) -> (Self, oneshot::Receiver<U>) {
        let (tx, rx) = oneshot::channel();
        let callback = Self {
            message,
            callback: tx,
        };
        (callback, rx)
    }
}
