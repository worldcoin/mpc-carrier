//! If you have a valid certificate for a domain name, and the domain name
//! resolves to your machine, you can run two instances of this example to test
//! inter-node communication:
//!
//! 1. `cargo run --example=node -- --cert-chain fullchain.pem --cert-priv-key \
//! privkey.pem 9000 <domainname>:9001`
//!
//! 2. `cargo run --example=node -- --cert-chain fullchain.pem --cert-priv-key \
//! privkey.pem 9001 <domainname>:9000`

#![warn(clippy::pedantic)]

use clap::Parser;
use mpc_carrier::channels::Callback;
use mpc_carrier::messages::{NodeRequest, NodeResponse};
use mpc_carrier::{Carrier, Error};
use std::time::Duration;
use std::{num::ParseIntError, path::PathBuf};
use thiserror::Error;
use tokio::time::sleep;
use tracing::info;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::EnvFilter;

#[derive(Error, Debug)]
pub enum NodeArgError {
    #[error("node argument doesn't have a colon")]
    NoColon,
    #[error("port parse error: {0}")]
    ParseIntError(#[from] ParseIntError),
}

#[derive(Debug, Parser)]
pub struct Cli {
    /// IP address to listen for incoming connections
    #[clap(long, default_value = "0.0.0.0")]
    pub bind: String,
    /// Certificate chain file
    #[clap(long)]
    pub cert_chain: PathBuf,
    /// Certificate private key file
    #[clap(long)]
    pub cert_priv_key: PathBuf,
    /// This node port.
    pub node_port: u16,
    /// Other nodes in form of domainname:port.
    #[clap(value_parser = parse_node)]
    pub nodes: Vec<(String, u16)>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let Cli {
        bind,
        cert_chain,
        cert_priv_key,
        node_port,
        nodes,
    } = Cli::parse();
    let filter = EnvFilter::default()
        .add_directive(LevelFilter::INFO.into())
        .add_directive(
            format!("mpc_carrier={}", LevelFilter::TRACE)
                .parse()
                .unwrap(),
        )
        .add_directive(
            format!("{}={}", env!("CARGO_CRATE_NAME"), LevelFilter::TRACE)
                .parse()
                .unwrap(),
        );
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let (carrier, mut incoming, mut outgoing) = Carrier::new(nodes.iter().cloned().collect());

    tokio::spawn(async move {
        let mut request_id = vec![0];
        let mut distance_list = vec![1, 2, 3, 4, 5, 6, 7, 8];
        distance_list.extend_from_slice(&node_port.to_be_bytes());
        loop {
            let request = NodeRequest {
                request_id: request_id.clone(),
                distance_list: distance_list.clone(),
            };
            for (node, _) in &nodes {
                info!("Sent {request:?} to {node}");
                let response = outgoing.send(node, request.clone()).await.unwrap();
                info!("Received {response:?} from {node}");
            }
            sleep(Duration::from_secs(1)).await;
            request_id[0] = request_id[0].wrapping_add(1);
            distance_list.rotate_right(1);
        }
    });

    tokio::spawn(async move {
        while let Some((node, Callback { message, callback })) = incoming.recv().await {
            info!("Received {message:?} from {node}");
            let response = NodeResponse {
                request_id: message.request_id.clone(),
            };
            info!("Sent {response:?} to {node}");
            callback.send(response).unwrap();
        }
    });

    carrier
        .run(&bind, node_port, &cert_chain, &cert_priv_key)
        .await
}

fn parse_node(s: &str) -> Result<(String, u16), NodeArgError> {
    let [node, port] = s
        .splitn(2, ':')
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| NodeArgError::NoColon)?;
    Ok((node.to_string(), port.parse()?))
}
