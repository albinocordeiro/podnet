use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use ed25519_dalek::{Signer, SigningKey};
use tracing::{debug, info};
use tokio::time::{interval, Duration};
use tokio::sync::broadcast;

use crate::replicaapi::{Transaction, Vote};
use crate::utils::get_current_round;
use crate::ROUND_SIZE_MS;

/// The heartbeat service periodically sends empty votes (heartbeats) to clients
/// This allows clients to detect if a replica is still active and functioning properly
pub async fn start_heartbeat_service(
    vote_sender: broadcast::Sender<Vote>,
    signing_key: SigningKey,
    replica_id: u64,
    nextsn: Arc<AtomicU64>,
) -> Result<()> {
    info!("Starting heartbeat service for replica {}", replica_id);
    
    // Create an interval timer for sending heartbeats
    info!("Creating interval timer for heartbeat service");
    let mut interval = interval(Duration::from_millis(ROUND_SIZE_MS));
    // Wait until the end of a round
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64;
    let current_round = current_time / ROUND_SIZE_MS;
    let next_round_start = (current_round + 1) * ROUND_SIZE_MS;
    let wait_time = next_round_start.saturating_sub(current_time);
    
    if wait_time > 0 {
        tokio::time::sleep(Duration::from_millis(wait_time)).await;
    }
    
    loop {
        // Wait for the next interval tick
        interval.tick().await;
        let sequence_number = nextsn.fetch_add(1, Ordering::SeqCst);
        
        // Create a heartbeat vote with a HEARTBEAT transaction
        let timestamp = get_current_round();
        let tx = b"HEARTBEAT".to_vec();
        
        // Create the message to sign (concatenate transaction data, timestamp, and sequence number)
        let mut message_to_sign = Vec::new();
        message_to_sign.extend_from_slice(&tx);
        message_to_sign.extend_from_slice(&timestamp.to_le_bytes());
        message_to_sign.extend_from_slice(&sequence_number.to_le_bytes());
        
        // Sign the message
        let signature = signing_key.sign(&message_to_sign);
        let sig = signature.to_bytes().to_vec();
        
        let heartbeat_vote = Vote {
            replica_id,
            timestamp,
            sequence_number,
            signature: sig,
            transaction: Some(Transaction { data: tx }),
        };
        
        // Send the heartbeat to all connected clients
        if vote_sender.send(heartbeat_vote).is_err() {
            info!("No clients connected to receive heartbeat from replica {}", replica_id);
        } else {
            debug!("Sent heartbeat from replica {} with sequence {}", replica_id, sequence_number);
        }
    }
}
