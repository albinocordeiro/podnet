use std::collections::{HashMap, BTreeMap};
use std::sync::Arc;

use anyhow::{bail, Result};
use ed25519_dalek::VerifyingKey;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, debug, error, warn};

use crate::{
    Replica, ReplicaId, SequenceNumber, Timestamp,
};
use crate::clientapi::{Transaction, Vote};

/// The task will be responsible for updating the local pod data based on the votes received from the replicas.
pub async fn start_pod_data_updater(
    mut vote_message_receiver: mpsc::Receiver<Vote>,
    replicas: Arc<RwLock<Box<HashMap<ReplicaId, Replica>>>>,
    tsps: Arc<RwLock<Box<HashMap<Transaction, HashMap<ReplicaId, Timestamp>>>>>,
    mrt: Arc<RwLock<Box<HashMap<ReplicaId, Timestamp>>>>,
    nextsn: Arc<RwLock<Box<HashMap<ReplicaId, SequenceNumber>>>>,
    ts_vote_msg: Arc<RwLock<Box<HashMap<Timestamp, Vote>>>>,
) -> Result<()> {
    info!("Starting pod view updater");
    
    // Backlog to store votes that arrive out of order, organized by replica and sequence number
    let mut backlog: HashMap<ReplicaId, BTreeMap<SequenceNumber, Vote>> = HashMap::new();
    
    // The task will be responsible for receiving vote messages from the vote_message channel and updating the pod view.
    while let Some(vote) = vote_message_receiver.recv().await {
        debug!("Updating pod view with vote message: {:?}", vote);
        
        let Vote {
            replica_id,
            timestamp,
            sequence_number,
            signature,
            transaction,
        } = vote.clone();
        
        info!("Received vote from replica: {:?}", replica_id);
        let Some(transaction) = transaction else {
            warn!("Received vote with missing transaction from replica {}, ignoring", replica_id);
            continue;
        };
        let replicas = replicas.read().await;
        let replica = match replicas.get(&replica_id) {
            Some(r) => r,
            None => {
                bail!("Bad initialization: Replica public key not found for replica id: {}", replica_id);
            }
        };
        
        // Verify signature Verify(pk, (tx, ts, sn), sig)
        if !verify_vote_signature(
            &replica.pk, 
            &transaction.data,
            timestamp, 
            sequence_number, 
            &signature
        ) {
            warn!("Signature verification failed for vote message: {:?}. Vote ignored.", vote);
            continue;
        }

        {
            let mut nextsn_lock = nextsn.write().await;
            let Some(nextsn) = nextsn_lock.get_mut(&replica_id) else {
                bail!("Bad initialization: Next sequence number not found for replica id: {}", &replica_id);
            };

            if sequence_number < *nextsn {
                // Vote is too old, discard it
                warn!("Received outdated vote with sequence number {} from replica {}, expected {}. Vote ignored.", 
                    sequence_number, replica_id, *nextsn);
                continue;
            } else if sequence_number > *nextsn {
                // Vote is from the future, store it in the backlog
                info!("Received future vote with sequence number {} from replica {}, expected {}. Adding to backlog.", 
                    sequence_number, replica_id, *nextsn);
                
                // Ensure the replica has a backlog entry
                if !backlog.contains_key(&replica_id) {
                    backlog.insert(replica_id, BTreeMap::new());
                }
                
                // Add the vote to the backlog
                if let Some(replica_backlog) = backlog.get_mut(&replica_id) {
                    replica_backlog.insert(sequence_number, vote.clone());
                }
                
                continue;
            }
            
            // At this point, sequence_number == *nextsn, so we process the vote
            *nextsn = *nextsn + 1;
        }
        
        // Process the vote - get a mrt write lock before processing
        {
            let mut mrt_lock = mrt.write().await;
            process_vote(replica_id, &transaction, timestamp, &mut mrt_lock, &tsps, ts_vote_msg.clone(), vote.clone()).await?;
        }
        
        // Try to process votes from the backlog for this replica
        if let Some(replica_backlog) = backlog.get_mut(&replica_id) {
            // Process consecutive sequence numbers from the backlog
            while let Some((&next_seq, _)) = replica_backlog.first_key_value() {
                let mut nextsn_lock = nextsn.write().await;
                let Some(nextsn) = nextsn_lock.get_mut(&replica_id) else {
                    bail!("Bad initialization: Next sequence number not found for replica id: {}", &replica_id);
                };
                
                if next_seq == *nextsn {
                    // We have the next expected vote in the backlog
                    let backlogged_vote = replica_backlog.remove(&next_seq).unwrap();
                    info!("Processing backlogged vote with sequence number {} from replica {}", next_seq, replica_id);
                    
                    // Increment the next sequence number
                    *nextsn = *nextsn + 1;
                    
                    // We need to drop the lock before processing the vote to avoid deadlocks
                    drop(nextsn_lock);
                    
                    // Extract vote data
                    let backlogged_transaction = match &backlogged_vote.transaction {
                        Some(tx) => tx,
                        None => {
                            warn!("Backlogged vote has missing transaction from replica {}, ignoring", replica_id);
                            continue;
                        }
                    };
                    
                    // Process the backlogged vote - get a new mrt write lock
                    {
                        let mut mrt_lock = mrt.write().await;
                        process_vote(replica_id, backlogged_transaction, 
                                   backlogged_vote.timestamp, &mut mrt_lock, &tsps, ts_vote_msg.clone(), backlogged_vote.clone()).await?;
                    }
                } else {
                    // No more consecutive votes in the backlog
                    break;
                }
            }
        }
    };

    Ok(())
}

/// Process a valid vote by updating the mrt and tsps
async fn process_vote(
    replica_id: ReplicaId,
    transaction: &Transaction,
    timestamp: Timestamp,
    mrt: &mut tokio::sync::RwLockWriteGuard<'_, Box<HashMap<ReplicaId, Timestamp>>>,
    tsps: &Arc<RwLock<Box<HashMap<Transaction, HashMap<ReplicaId, Timestamp>>>>>,
    ts_vote_msg: Arc<RwLock<Box<HashMap<Timestamp, Vote>>>>,
    vote: Vote
) -> Result<()> {
    {
        let mut ts_vote_msg_lock = ts_vote_msg.write().await;
        if !ts_vote_msg_lock.contains_key(&timestamp) {
            ts_vote_msg_lock.insert(timestamp, vote.clone());
        }
    }
    let Some(mrt_entry) = mrt.get_mut(&replica_id) else {
        bail!("Bad initialization: MRT not found for replica id: {}", replica_id);
    };

    if timestamp < *mrt_entry {
        error!("Replica sent old timestamp for replica id: {}. Vote ignored", replica_id);
        return Ok(());
    }

    *mrt_entry = timestamp;

    // if tx is HEARTBEAT skip updating tsps
    if !is_heartbeat(transaction) {
        debug!("Updating timestamps for transaction: {:?}", transaction);
        let mut tsps_lock = tsps.write().await;
        
        // Create the transaction entry in tsps if it doesn't exist
        if !tsps_lock.contains_key(transaction) {
            tsps_lock.insert(transaction.clone(), HashMap::new());
        }
        
        let Some(replica_timestamp_map) = tsps_lock.get_mut(transaction) else {
            bail!("Failed to get or create timestamp map for transaction: {:?}", transaction);
        };

        if replica_timestamp_map.contains_key(&replica_id) && replica_timestamp_map[&replica_id] != timestamp {
            info!("Duplicate timestamp from replica {} for transaction {:?}, ignoring", replica_id, transaction);
            return Ok(());
        }

        replica_timestamp_map.insert(replica_id, timestamp);
    } else {
        debug!("Received heartbeat from replica {}", replica_id);
    }
    
    Ok(())
}

/// Checks if a transaction is a heartbeat message
fn is_heartbeat(tx: &Transaction) -> bool {
    info!("Checking if transaction is heartbeat");
    tx.data.as_slice() == b"HEARTBEAT"
}

/// Verifies the vote message signature using the replica's public key.
/// Constructs a message consisting of transaction data, timestamp, and sequence number,
/// then verifies that the signature matches this data when signed with the replica's key.
fn verify_vote_signature(pk: &VerifyingKey, transaction: &[u8], timestamp: u64, sequence_number: u64, signature: &[u8]) -> bool {
    // Create a message to verify by concatenating:
    // - transaction data
    // - timestamp (as 8 bytes)
    // - sequence_number (as 8 bytes)
    let mut message = Vec::with_capacity(transaction.len() + 16);
    message.extend_from_slice(transaction);
    message.extend_from_slice(&timestamp.to_le_bytes());
    message.extend_from_slice(&sequence_number.to_le_bytes());
    
    // Convert the signature bytes to a Signature
    // If conversion fails, return false (invalid signature format)
    if signature.len() != ed25519_dalek::Signature::BYTE_SIZE {
        warn!("Invalid signature length: expected {} but got {}", 
            ed25519_dalek::Signature::BYTE_SIZE, signature.len());
        return false;
    }
    
    let signature_array: [u8; ed25519_dalek::Signature::BYTE_SIZE] = match signature.try_into() {
        Ok(arr) => arr,
        Err(_) => {
            warn!("Failed to convert signature to fixed-size array");
            return false;
        }
    };
    
    let signature = match ed25519_dalek::Signature::try_from(signature_array) {
        Ok(sig) => sig,
        Err(e) => {
            warn!("Invalid signature format: {}", e);
            return false;
        }
    };
    
    // Verify the signature
    match pk.verify_strict(&message, &signature) {
        Ok(_) => true,
        Err(e) => {
            warn!("Signature verification failed: {}", e);
            false
        }
    }
}
