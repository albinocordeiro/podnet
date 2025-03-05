use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use ed25519_dalek::{Signer, SigningKey};
use tokio::sync::{broadcast, mpsc};
use tracing::{info, error};

use crate::replica_log::ReplicaLogAppender;
use crate::replicaapi::{Transaction, VoteRecord, Vote};
use crate::utils::get_current_round;

/// Start the log and vote service
/// 
/// This service is responsible for:
/// 1. Receiving transactions from the write channel
/// 2. Persisting them as VoteRecords
/// 3. Broadcasting Votes to clients
pub async fn start_log_vote_service(
    mut write_receiver: mpsc::Receiver<Transaction>,
    vote_sender: broadcast::Sender<Vote>,
    replica_log: ReplicaLogAppender<VoteRecord>,
    signing_key: SigningKey,
    replica_id: u64,
    nextsn: Arc<AtomicU64>,
) -> Result<()> {
    info!("Starting log and vote service");

    // Process incoming transactions
    while let Some(transaction) = write_receiver.recv().await {
        // Get the next sequence number
        let sn = nextsn.fetch_add(1, Ordering::SeqCst);
        let ts = get_current_round();
        
        // Create the message to sign (concatenate transaction data, timestamp, and sequence number)
        let mut message_to_sign = Vec::new();
        message_to_sign.extend_from_slice(&transaction.data);
        message_to_sign.extend_from_slice(&ts.to_le_bytes());
        message_to_sign.extend_from_slice(&sn.to_le_bytes());
        
        // Sign the message
        let signature = signing_key.sign(&message_to_sign);
        let sig = signature.to_bytes().to_vec();
        
        // Create a VoteRecord
        let vote_record = VoteRecord {
            tx: Some(transaction.clone()),
            ts,
            sn,
            sig: sig.clone(), 
        };
        
        // Log the VoteRecord to the persistent store
        if let Err(e) = replica_log.append(&vote_record) {
            error!("Failed to write vote record to log: {}", e);
            continue;
        }
        
        // Create a Vote message to broadcast
        let vote = Vote {
            replica_id,
            transaction: Some(transaction.clone()),
            timestamp: ts,
            sequence_number: sn,
            signature: sig, 
        };
        
        // Broadcast the vote
        if let Err(e) = vote_sender.send(vote) {
            error!("Failed to broadcast vote: {}", e);
        } else {
            info!("Broadcasted vote with sequence number {} to {} receivers", 
                  sn, vote_sender.receiver_count());
        }
    }
    
    info!("Log and vote service stopped");
    Ok(())
}
