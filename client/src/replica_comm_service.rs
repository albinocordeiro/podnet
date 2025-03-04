use anyhow::{Context, Result, bail};
use std::time::Duration;
use futures::{SinkExt, StreamExt};
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tracing::{warn, debug, error, info};

use crate::{Replica, clientapi::Transaction, clientapi::Vote};

pub async fn start_replica_comm_service(
    replicas: Vec<Replica>,
    write_request_receiver: tokio::sync::broadcast::Receiver<Transaction>,
    vote_message_sender: tokio::sync::mpsc::Sender<Vote>,
) -> Result<()> {
    info!("Starting replica communication service");

    let mut join_handles = Vec::new();

    for replica in replicas.iter() {
        let replica_id = replica.id;
        let replica_endpoint = replica.endpoint.url.clone();
        let write_request_recv = write_request_receiver.resubscribe();
        let vote_message_sender = vote_message_sender.clone();

        let join_handle = tokio::spawn(async move {
            info!("Connecting to replica: {}", replica_id);
            let ws_stream = timeout(Duration::from_secs(5), connect_async(replica_endpoint))
                .await
                .context(format!("Failed to connect to replica {} with reasonable time", replica_id))?
                .context(format!("Failed to connect to replica {}", replica_id))?;
            
            let (mut ws_write, mut ws_read) = ws_stream.0.split();
            let mut write_request_recv = write_request_recv.resubscribe();

            loop {
                tokio::select! {
                    Ok(transaction) = write_request_recv.recv() => {
                        ws_write
                            .send(tungstenite::Message::Binary(transaction.data.clone()))
                            .await
                            .context(format!("Failed to send message to replica {}", replica_id))?;
                        debug!("Sent write request to replica {}: {:?}", replica_id, transaction);
                        
                    }
                    msg = ws_read.next() => {
                        match msg {
                            Some(Ok(message)) => {
                                // Process the message from the replica
                                match message {
                                    tungstenite::Message::Binary(binary) => {
                                        let vote_msg: Vote = prost::Message::decode(binary.as_slice())
                                        .context(format!("Failed to parse Vote message from replica {}", replica_id))?;
                                        vote_message_sender.send(vote_msg.clone()).await
                                            .context(format!("Failed to send vote message from replica {}", replica_id))?;
                                        debug!("Received vote message from replica {}: {:?}", replica_id, vote_msg);
                                    }
                                    _ => {
                                        warn!("Unexpected message type from replica {}: {:?}", replica_id, message);
                                    }
                                }

                            }
                            Some(Err(e)) => {
                                error!("Error from replica {}: {:?}", replica_id, e);
                                break;
                            }
                            None => {
                                info!("Connection closed with (by) replica {}", replica_id);
                                break;
                            }
                        }
                    }
                }
            }
            info!("Exiting comms loop with replica {}", replica_id);
            Ok::<(), anyhow::Error>(())
        });
        join_handles.push(join_handle);
    }

    // Await all tasks to complete.
    let results = futures::future::join_all(join_handles).await;
    for result in results.iter() {
        if let Err(e) = result {
            error!("Error in websockets supervisor: {:?}", e);
        }
    }
    if results.iter().any(|r| r.is_err()) {
        bail!("Websockets ");
    }
    info!("Websockets supervisor closing without errors...");
    Ok(())
}

