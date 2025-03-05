use std::path::PathBuf;

use anyhow::Result;
use prost::Message as _;
use tokio::sync::{broadcast, mpsc};
use tracing::{info, error};
use futures_util::{StreamExt, SinkExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use crate::replica_log::get_replica_log_iterator;
use crate::replicaapi::{Transaction, Vote, VoteRecord};

const CHANNEL_SIZE: usize = 100;

/// Start the client communications service
/// 
/// This service is responsible for:
/// 1. Accepting WebSocket connections from clients
/// 2. Processing transactions received from clients
/// 3. Sending votes back to clients
/// 
pub async fn start_client_comms_service(
    write_sender: mpsc::Sender<Transaction>,
    vote_receiver: broadcast::Receiver<Vote>,
    replica_log_file: &str,
    replica_id: u64,
    port: u16,
) -> Result<()> {
    info!("Starting client communications service on port {}", port);
    
    // Accept WebSocket connections
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    
    info!("WebSocket server listening on: {}", addr);
    
    // Handle incoming connections
    while let Ok((stream, addr)) = listener.accept().await {
        info!("Incoming TCP connection from: {}", addr);
        
        // Clone resources for this connection
        let write_sender = write_sender.clone();
        let connection_vote_receiver = vote_receiver.resubscribe();
        let connection_replica_log_file = replica_log_file.to_string();
        
        tokio::spawn(async move {
            match tokio_tungstenite::accept_async(stream).await {
                Ok(ws_stream) => {
                    info!("Client connection established");
                    if let Err(e) = handle_client_connection(
                        ws_stream, 
                        write_sender, 
                        connection_vote_receiver, 
                        replica_id,
                        &connection_replica_log_file
                    ).await {
                        error!("Error while handling client connection: {}", e);
                    }
                }
                Err(e) => {
                    error!("Error during WebSocket handshake: {}", e);
                }
            }
        });
    }
    
    Ok(())
}

/// Handle a single client WebSocket connection
///
/// This function processes messages from a client and sends votes to the client.
/// It runs until the client disconnects or an error occurs.
async fn handle_client_connection(
    ws_stream: WebSocketStream<TcpStream>,
    write_sender: mpsc::Sender<Transaction>,
    mut vote_receiver: broadcast::Receiver<Vote>,
    replica_id: u64,
    replica_log_file: &str,
) -> Result<()> {
    info!("Handling new client connection for replica_id: {}", replica_id);
    
    // Split the WebSocket stream into a sink and a stream
    let (mut ws_sink, mut ws_stream) = ws_stream.split();
    
    // Spawn a task to handle incoming messages from the client
    let write_handle = tokio::spawn(async move {
        while let Some(msg_result) = ws_stream.next().await {
            match msg_result {
                Ok(msg) => {
                    match msg {
                        Message::Binary(data) => {
                            // Try to decode as a Transaction
                            let tx = Transaction{
                                data,
                            };
                            if let Err(e) = write_sender.send(tx).await {
                                error!("Failed to forward transaction: {}", e);
                            }
                        },
                        Message::Close(_) => {
                            info!("Client sent close message");
                            break;
                        },
                        _ => {
                            // Ignore other message types
                        }
                    }
                },
                Err(e) => {
                    error!("Error receiving message from client: {}", e);
                    break;
                }
            }
        }
        info!("Client message handler exited");
    });

    // Create a channel for votes
    // a non broadcast channel to merge new votes (from the broadcast channel) and historical votes from the WAL
    let (vote_tx, mut vote_rx) = mpsc::channel::<Vote>(CHANNEL_SIZE);

    let vote_tx_for_funnel = vote_tx.clone();
    let vote_funnel_handle = tokio::spawn(async move {
        // Handle forwarding votes from broadcast channel to mpsc channel
        info!("Starting vote funnel task");
        while let Ok(vote) = vote_receiver.recv().await {
            if let Err(e) = vote_tx_for_funnel.send(vote).await {
                error!("Failed to forward vote to client handler: {}", e);
                if vote_tx_for_funnel.is_closed() {
                    info!("Vote channel closed, stopping vote funnel");
                    break;
                }
            }
        }
        info!("Vote funnel task exited");
    });
    
    let replica_log_file_path = PathBuf::from(replica_log_file);
    let _catch_up_handle = tokio::spawn(async move {
        // Send historical votes from the replica log (concurrently)
        info!("Sending historical votes from replica log");
        let log_iterator = match get_replica_log_iterator::<VoteRecord>(replica_log_file_path) {
            Ok(iter) => iter,
            Err(e) => {
                error!("Failed to create replica log iterator: {}", e);
                return;
            }
        };

        for record_result in log_iterator {
            match record_result {
                Ok(vote_record) => {
                    // Convert VoteRecord to Vote
                    let vote = Vote {
                        replica_id,
                        timestamp: vote_record.ts,
                        sequence_number: vote_record.sn,
                        signature: vote_record.sig,
                        transaction: vote_record.tx,
                    };
                    
                    // Send the vote to the client
                    if let Err(e) = vote_tx.send(vote).await {
                        error!("Failed to send historical vote to client: {}", e);
                        break;
                    }
                },
                Err(e) => {
                    error!("Error reading from replica log: {}", e);
                    break;
                }
            }
        }
        info!("Finished sending historical votes");
    });

    // Forward votes to the client
    let vote_handle = tokio::spawn(async move {        
        info!("Starting to forward new votes to client");
        
        while let Some(vote) = vote_rx.recv().await {
            // Create a copy of the vote with the correct replica_id
            let mut vote_with_replica_id = vote.clone();
            vote_with_replica_id.replica_id = replica_id;
            
            // Encode the vote
            let mut buf = Vec::new();
            if let Err(e) = vote_with_replica_id.encode(&mut buf) {
                error!("Failed to encode vote: {}", e);
                continue;
            }
            
            // Send the vote to the client
            if let Err(e) = ws_sink.send(Message::Binary(buf)).await {
                error!("Failed to send vote to client: {}", e);
                break;
            }
        }
        info!("Vote sender exited");
    });

    // Wait for any task to complete
    tokio::select! {
        _ = write_handle => info!("Write handler completed"),
        _ = vote_handle => info!("Vote handler completed"),
        _ = vote_funnel_handle => info!("Vote funnel task completed"),
    }
    
    info!("Client connection handler exited");
    Ok(())
}