//! Worldcoin MPC communication channel.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

pub mod channels;
pub mod node;
pub mod protobuf_tcp;
pub mod tls;

/// Communication messages.
#[allow(missing_docs)]
pub mod messages {
    include!(concat!(env!("OUT_DIR"), "/messages.rs"));
}

const CHANNEL_CAPACITY: usize = 64;

use channels::{Incoming, NodeCallback, Outgoing};
use futures::channel::mpsc;
use futures::future;
use futures::prelude::*;
use rustls::pki_types::ServerName;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::task;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::TlsConnector;
use tokio_stream::wrappers::TcpListenerStream;
use tracing::info;

/// Service error.
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum Error {
    #[error("TLS initialization: {0}")]
    TlsInit(#[from] tls::Error),
    #[error("socket: {0}")]
    Socket(io::Error),
}

/// Communication worker.
pub struct Carrier {
    nodes: HashMap<String, u16>,
    incoming: HashMap<String, mpsc::Sender<NodeCallback>>,
    outgoing: HashMap<String, mpsc::Receiver<NodeCallback>>,
}

impl Carrier {
    /// Creates a new [`Carrier`] together with an associated [`Incoming`] and
    /// [`Outgoing`] channel sets.
    #[must_use]
    pub fn new(nodes: HashMap<String, u16>) -> (Self, Incoming, Outgoing) {
        let (mut incoming_tx, mut incoming_rx) = (HashMap::new(), HashMap::new());
        let (mut outgoing_tx, mut outgoing_rx) = (HashMap::new(), HashMap::new());
        for node in nodes.keys() {
            let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
            incoming_tx.insert(node.clone(), tx);
            incoming_rx.insert(node.clone(), rx);
            let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
            outgoing_tx.insert(node.clone(), tx);
            outgoing_rx.insert(node.clone(), rx);
        }
        let carrier = Self {
            nodes,
            incoming: incoming_tx,
            outgoing: outgoing_rx,
        };
        let incoming = Incoming::new(incoming_rx);
        let outgoing = Outgoing::new(outgoing_tx);
        (carrier, incoming, outgoing)
    }

    /// Runs the communication.
    pub async fn run(
        self,
        bind: &str,
        node_port: u16,
        cert_chain: &Path,
        cert_priv_key: &Path,
    ) -> Result<(), Error> {
        let Self {
            nodes,
            incoming,
            mut outgoing,
        } = self;
        let mut futures = Vec::new();
        let (server_config, client_config) = tls::init(cert_chain, cert_priv_key)?;

        let acceptor = TlsAcceptor::from(server_config);
        futures.push(listen(bind, node_port, acceptor, (incoming,), node::incoming).boxed());

        for (node, port) in nodes {
            let connector = TlsConnector::from(Arc::clone(&client_config));
            let dnsname = ServerName::try_from(node.clone()).unwrap();
            let outgoing = outgoing.remove(&node).unwrap();
            futures.push(node::outgoing(node, port, connector, dnsname, outgoing).boxed());
        }

        let (result, _, _) = future::select_all(futures).await;
        result
    }
}

async fn listen<A, F, T>(
    ip: &str,
    port: u16,
    acceptor: TlsAcceptor,
    args: A,
    mut serve: F,
) -> Result<(), Error>
where
    A: Clone,
    F: FnMut(TcpStream, TlsAcceptor, A) -> T,
    T: Future<Output = Result<(), Error>> + Send + 'static,
{
    let listener = TcpListener::bind((ip, port)).await.map_err(Error::Socket)?;
    let listener = TcpListenerStream::new(listener);
    info!("Listening for incoming connections to {ip}:{port}");
    listener
        .map_err(Error::Socket)
        .try_for_each_concurrent(None, |sock| {
            task::spawn(serve(sock, acceptor.clone(), args.clone()))
                .map(|r| r.expect("connection task failure"))
        })
        .await?;
    Ok(())
}
