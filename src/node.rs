//! Node-to-node communication.

use crate::channels::Callback;
use crate::{messages, protobuf_tcp, NodeCallback};
use async_stream::try_stream;
use futures::channel::{mpsc, oneshot};
use futures::future::{self, Either};
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use rustls::pki_types::ServerName;
use std::pin::pin;
use std::time::Duration;
use std::{collections::HashMap, io};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing::{debug, error, instrument, trace};

const MAX_LEN: usize = 8 * 1024 * 1024;
const OUTGOING_CONNECTION_RETRY_INTERVAL: Duration = Duration::from_millis(200);

/// Node-to-node communication error.
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum Error {
    #[error("TLS: {0}")]
    Tls(io::Error),
    #[error("Socket: {0}")]
    Socket(io::Error),
    #[error("SNI failure")]
    Sni,
    #[error("Unknown server name")]
    UnknownServerName,
    #[error("Protocol: {0}")]
    Protocol(#[from] protobuf_tcp::Error),
    #[error("Unexpected response with request_id: {0:?}")]
    UnexpectedResponse(Vec<u8>),
}

/// Handles a new incoming node-to-node connection.
#[instrument(name = "node-incoming", level = "error", skip_all)]
pub async fn incoming(
    sock: TcpStream,
    acceptor: TlsAcceptor,
    (incoming,): (HashMap<String, mpsc::Sender<NodeCallback>>,),
) -> Result<(), crate::Error> {
    match serve_incoming(sock, acceptor, incoming).await {
        Ok(()) => Ok(()),
        Err(err) => {
            debug!("Connection terminated: {err}");
            Ok(())
        }
    }
}

/// Handles an outgoing node-to-node connection.
#[instrument(name = "node-outgoing", level = "error", skip_all)]
pub async fn outgoing(
    node: String,
    port: u16,
    connector: TlsConnector,
    dnsname: ServerName<'static>,
    mut outgoing: mpsc::Receiver<NodeCallback>,
) -> Result<(), crate::Error> {
    loop {
        if let Err(err) =
            serve_outgoing(node.clone(), port, &connector, &dnsname, &mut outgoing).await
        {
            debug!("Connection failure: {err}");
        }
        sleep(OUTGOING_CONNECTION_RETRY_INTERVAL).await;
    }
}

async fn serve_incoming(
    sock: TcpStream,
    acceptor: TlsAcceptor,
    mut incoming: HashMap<String, mpsc::Sender<NodeCallback>>,
) -> Result<(), Error> {
    let stream = acceptor.accept(sock).await.map_err(Error::Tls)?;
    let server_name = stream.get_ref().1.server_name().ok_or(Error::Sni)?;
    trace!("Accepted a new connection from {server_name}");
    let incoming = incoming
        .get_mut(server_name)
        .ok_or(Error::UnknownServerName)?;
    let (reader, mut writer) = protobuf_tcp::new(stream.into(), MAX_LEN);

    let mut callbacks = FuturesUnordered::new();
    let mut incoming_requests = pin!(incoming_requests(reader, incoming));
    loop {
        match future::select(incoming_requests.next(), callbacks.next()).await {
            Either::Left((Some(rx), _)) => {
                callbacks.push(rx?);
            }
            Either::Left((None, _)) => return Ok(()),
            Either::Right((Some(callback), _)) => {
                if let Ok(response) = callback {
                    writer.write::<messages::NodeResponse>(response).await?;
                    writer.flush().await?;
                }
            }
            Either::Right((None, _)) => {}
        }
    }
}

async fn serve_outgoing(
    node: String,
    port: u16,
    connector: &TlsConnector,
    dnsname: &ServerName<'static>,
    outgoing: &mut mpsc::Receiver<NodeCallback>,
) -> Result<(), Error> {
    let stream = TcpStream::connect((node.clone(), port))
        .await
        .map_err(Error::Socket)?;
    let stream = connector
        .connect(dnsname.clone(), stream)
        .await
        .map_err(Error::Tls)?;
    trace!("Established a connection to {node}:{port}");
    let (reader, mut writer) = protobuf_tcp::new(stream.into(), MAX_LEN);

    let mut callbacks = HashMap::new();
    let mut incoming_responses = pin!(incoming_responses(reader));
    loop {
        match future::select(outgoing.next(), incoming_responses.next()).await {
            Either::Left((None, _)) | Either::Right((None, _)) => return Ok(()),
            Either::Left((Some(Callback { message, callback }), _)) => {
                if callbacks
                    .insert(message.request_id.clone(), callback)
                    .is_none()
                {
                    writer.write::<messages::NodeRequest>(message).await?;
                    writer.flush().await?;
                } else {
                    error!("Colliding request_id: {:?}", message.request_id);
                }
            }
            Either::Right((Some(message), _)) => {
                let message = message?;
                if let Some(callback) = callbacks.remove(&message.request_id) {
                    let _ = callback.send(message);
                } else {
                    Err(Error::UnexpectedResponse(message.request_id))?;
                }
            }
        }
    }
}

fn incoming_requests(
    mut reader: protobuf_tcp::Reader,
    incoming: &mut mpsc::Sender<NodeCallback>,
) -> impl Stream<Item = Result<oneshot::Receiver<messages::NodeResponse>, Error>> + '_ {
    try_stream! {
        loop {
            let message = reader.read::<messages::NodeRequest>().await?;
            let (message, rx) = Callback::new(message);
            incoming.send(message).await.expect("to be alive");
            yield rx;
        }
    }
}

fn incoming_responses(
    mut reader: protobuf_tcp::Reader,
) -> impl Stream<Item = Result<messages::NodeResponse, Error>> {
    try_stream! {
        loop {
            let message = reader.read::<messages::NodeResponse>().await?;
            yield message;
        }
    }
}
