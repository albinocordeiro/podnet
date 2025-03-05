use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{broadcast, mpsc};
use tokio::spawn;
use ed25519_dalek::SigningKey;

use tracing::{error, info};

use crate::replica_log::ReplicaLogAppender;
use crate::client_comms_service::start_client_comms_service;
use crate::log_vote_service::start_log_vote_service;
use crate::replicaapi::Vote;
use crate::heartbeat_service::start_heartbeat_service;

const CHANNEL_SIZE: usize = 100;

pub async fn init_pod_network_node(replica_id: u64, port: u16, replica_log_file: &str, signing_key: SigningKey) -> Result<()> {
    info!("Initializing the pod network node (Replica)");
    
    let (vote_sender, vote_receiver) = broadcast::channel::<Vote>(CHANNEL_SIZE);
    let (write_sender, write_receiver) = mpsc::channel(CHANNEL_SIZE);

    // Create owned copies of the data that will be moved into the spawned task
    let replica_log_file_owned = replica_log_file.to_string();
    
    let client_comms = spawn(async move {
        if let Err(e) = start_client_comms_service(
            write_sender,
            vote_receiver,
            &replica_log_file_owned,
            replica_id,
            port,
        ).await {
            error!("Client communications service error: {}", e);
        }
    });
    let nextsn = Arc::new(AtomicU64::new(0));
    let replica_log_appender = ReplicaLogAppender::open(replica_log_file)?;
    let vote_sender_log_service = vote_sender.clone();
    let signing_key_log_service = signing_key.clone();
    let nextsn_log_service = nextsn.clone();
    // Clone is no longer needed here because we're using the original
    let log_service = spawn(async move {
        if let Err(e) = start_log_vote_service(
            write_receiver,
            vote_sender_log_service,
            replica_log_appender,
            signing_key_log_service,
            replica_id,
            nextsn_log_service,
        ).await {
            error!("Log and vote service error: {}", e);
        }
    });

    // Heartbeat service
    let heartbeat_service = spawn(async move {
        if let Err(e) = start_heartbeat_service(
            vote_sender,
            signing_key,
            replica_id,
            nextsn,
        ).await {
            error!("Heartbeat service error: {}", e);
        }
    });

    // Wait for both tasks to complete
    tokio::select! {
        client_result = client_comms => {
            match client_result {
                Ok(_) => info!("Client communications service task completed"),
                Err(e) => error!("Client communications service task error: {}", e),
            }
        }
        log_result = log_service => {
            match log_result {
                Ok(_) => info!("Log and vote service task completed"),
                Err(e) => error!("Log and vote service task error: {}", e),
            }
        }
        heartbeat_result = heartbeat_service => {
            match heartbeat_result {
                Ok(_) => info!("Heartbeat service task completed"),
                Err(e) => error!("Heartbeat service task error: {}", e),
            }
        }
    }

    info!("Exiting pod network node (Replica)");
    Ok(())
}
