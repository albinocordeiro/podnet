use anyhow::{Context, Result, bail};
use url::Url;
use std::time::Duration;
use futures::{SinkExt, StreamExt};
use tokio::time::timeout;
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, WebSocketStream, MaybeTlsStream};
use tracing::{warn, debug, error, info};

use crate::{Replica, clientapi::Transaction, clientapi::Vote};

/// Process results from connection tasks and report any errors.
///
/// Logs all errors and returns an error if any task failed.
fn process_connection_results(
    results: Vec<Result<Result<(), anyhow::Error>, tokio::task::JoinError>>
) -> Result<()> {
    for result in results.iter() {
        match result {
            Ok(r) => {
                match r {
                    Ok(_) => {},
                    Err(e) => {
                        error!("{:?}", e);
                    }
                }
            },
            Err(e) => {
                error!("Error in replica communication task: {:?}", e);
            }
        }
    }
    if results.iter().any(|r| r.is_err() || r.as_ref().unwrap().is_err()) {
        bail!("Replica Communication Service closed with errors");
    }
    Ok(())
}

/// Establishes a WebSocket connection to a replica with retry logic.
///
/// Attempts to connect to the replica endpoint multiple times before giving up.
async fn connect_to_replica(
    replica_endpoint: &Url
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    info!("Connecting to replica");
    let ws_stream;
    let mut attempts = 0;
    loop {
        attempts += 1;
        if attempts > 5 {
            bail!("Could not connect to replica after 5 attempts, giving up");
        }
        match timeout(Duration::from_secs(5), connect_async(replica_endpoint))
            .await
        {
            Ok(ws) => {
                ws_stream = match ws {
                    Ok(ws) => ws.0,
                    Err(e) => {
                        bail!("Websocket connection error: {}", e);
                    }
                };
                break;
            }
            Err(_) => {
                warn!("Could not connect to replica after reasonable time, retrying in 1 second");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    
    Ok(ws_stream)
}

/// Handles connection and communication with a single replica.
///
/// Establishes a WebSocket connection with the replica, sends transactions,
/// and processes vote messages in a continuous loop.
async fn handle_replica_connection(
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    write_request_recv: tokio::sync::broadcast::Receiver<Transaction>,
    vote_message_sender: tokio::sync::mpsc::Sender<Vote>,
) -> Result<()> {
    let (mut ws_write, mut ws_read) = ws_stream.split();

    let mut write_request_recv = write_request_recv.resubscribe();
    
    info!("Starting replica communication loop");
    loop {
        tokio::select! {
            Ok(transaction) = write_request_recv.recv() => {
                ws_write
                    .send(tungstenite::Message::Binary(transaction.data.clone()))
                    .await
                    .context("Failed to send message to replica")?;
                info!("Sent write request to replica: {:?}", transaction);
                
            }
            msg = ws_read.next() => {
                match msg {
                    Some(Ok(message)) => {
                        // Process the message from the replica
                        match message {
                            tungstenite::Message::Binary(binary) => {
                                let vote_msg: Vote = prost::Message::decode(binary.as_slice())
                                .context("Failed to parse Vote message from replica")?;
                                vote_message_sender.send(vote_msg.clone()).await
                                    .context("Failed to send vote message from replica")?;
                                debug!("Received vote message from replica: {:?}", vote_msg);
                            }
                            _ => {
                                warn!("Unexpected message type from replica: {:?}", message);
                            }
                        }

                    }
                    Some(Err(e)) => {
                        error!("Error from replica: {:?}", e);
                        break;
                    }
                    None => {
                        info!("Connection closed with (by) replica");
                        break;
                    }
                }
            }
        }
    }
    info!("Exiting comms loop with replica");
    Ok::<(), anyhow::Error>(())
}

pub async fn start_replica_comms_service(
    replicas: Vec<Replica>,
    write_request_receiver: tokio::sync::broadcast::Receiver<Transaction>,
    vote_message_sender: tokio::sync::mpsc::Sender<Vote>,
) -> Result<()> {
    info!("Starting replica communication service");

    let mut join_handles = Vec::new();

    for replica in replicas.iter() {
        let replica_id = replica.id.clone();
        let replica_endpoint = replica.url.clone();
        let write_request_recv = write_request_receiver.resubscribe();
        let vote_message_sender = vote_message_sender.clone();

        let join_handle: tokio::task::JoinHandle<Result<(), anyhow::Error>> = tokio::spawn(async move {
            let ws_stream = connect_to_replica(&replica_endpoint)
                .await
                .context(format!("Failed to connect to replica {}", replica_id))?;
            handle_replica_connection(ws_stream, write_request_recv, vote_message_sender)
                .await
                .context(format!("Failed to handle replica connection {}", replica_id))?;
            Ok::<(), anyhow::Error>(())
        });
        join_handles.push(join_handle);
    }

    // Await all tasks to complete.
    let results = futures::future::join_all(join_handles).await;
    process_connection_results(results)?;

    info!("Replica Communication Service closing without errors...");
    Ok(())
}
