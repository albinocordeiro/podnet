use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Result};
use ed25519_dalek::VerifyingKey;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{info, debug, error, warn};

use crate::client_api_service::start_api_server;
use crate::replica_comm_service::start_replica_comm_service;

use crate::{
    Replica, ReplicaId, SequenceNumber, Timestamp,
};
use crate::clientapi::{Transaction, Vote};
pub const CHANNEL_SIZE: usize = 100;

#[derive(Debug)]
pub struct PodNetworkClient {
    pub replicas: Arc<RwLock<Box<HashMap<ReplicaId, Replica>>>>,
    pub tsps: Arc<RwLock<Box<HashMap<Transaction, HashMap<ReplicaId, Timestamp>>>>>,
    pub mrt: Arc<RwLock<Box<HashMap<ReplicaId, Timestamp>>>>,
    pub nextsn: Arc<RwLock<Box<HashMap<ReplicaId, SequenceNumber>>>>,
    pub ts_vote_msg: Arc<RwLock<Box<HashMap<Timestamp, Vote>>>>,
}

impl PodNetworkClient {
    pub fn new() -> PodNetworkClient {
        PodNetworkClient {
            replicas: Arc::new(RwLock::new(Box::new(HashMap::new()))),
            tsps: Arc::new(RwLock::new(Box::new(HashMap::new()))),
            mrt: Arc::new(RwLock::new(Box::new(HashMap::new()))),
            nextsn: Arc::new(RwLock::new(Box::new(HashMap::new()))),
            ts_vote_msg: Arc::new(RwLock::new(Box::new(HashMap::new()))),
        }
    }

    pub async fn init(&mut self, replicas: Vec<Replica>) -> Result<()> {
        info!("Initializing the pod network client");
        self.replicas = Arc::new(RwLock::new(Box::new(replicas.iter().cloned().map(|replica| (replica.id, replica)).collect())));
        let mut mrt = HashMap::new();
        let tsps = HashMap::new();
        let mut nextsn = HashMap::new();
        let ts_vote_msg = HashMap::new();

        for replica in self.replicas.read().await.values() {
            mrt.insert(replica.id, 0);
            nextsn.insert(replica.id, -1);
        }

        self.mrt = Arc::new(RwLock::new(Box::new(mrt)));
        self.tsps = Arc::new(RwLock::new(Box::new(tsps)));
        self.nextsn = Arc::new(RwLock::new(Box::new(nextsn)));
        self.ts_vote_msg = Arc::new(RwLock::new(Box::new(ts_vote_msg)));

        let (write_request_sender, write_request_receiver) = broadcast::channel(CHANNEL_SIZE);
        let (vote_message_sender, vote_message_receiver) = mpsc::channel(CHANNEL_SIZE);

        let rpc_api_server = start_api_server(
            write_request_sender, 
            self.replicas.clone(),
            self.tsps.clone(),
            self.mrt.clone(),
            self.nextsn.clone(),
            self.ts_vote_msg.clone(),
        );

        let pod_view_updater = start_pod_data_updater(
            vote_message_receiver, 
            self.replicas.clone(),
            self.tsps.clone(),
            self.mrt.clone(),
            self.nextsn.clone(),
        );

        let replica_comms = start_replica_comm_service(
            replicas, 
            write_request_receiver, 
            vote_message_sender
        );

        let (api_result, pod_updater_result, websockets_supervisor_result) =
            tokio::join!(rpc_api_server, pod_view_updater, replica_comms);
        if api_result.is_err() {
            error!("Error running the API server: {:?}", api_result.err());
        }
        if pod_updater_result.is_err() {
            error!(
                "Error starting the pod view updater: {:?}",
                pod_updater_result.err()
            );
        }
        if websockets_supervisor_result.is_err() {
            error!(
                "Error starting the replica comms service: {:?}",
                websockets_supervisor_result.err()
            );
        }

        Ok(())
    }
}

/// The task will be responsible for updating the local pod data based on the votes received from the replicas.
async fn start_pod_data_updater(
    mut vote_message_receiver: mpsc::Receiver<Vote>,
    replicas: Arc<RwLock<Box<HashMap<ReplicaId, Replica>>>>,
    tsps: Arc<RwLock<Box<HashMap<Transaction, HashMap<ReplicaId, Timestamp>>>>>,
    mrt: Arc<RwLock<Box<HashMap<ReplicaId, Timestamp>>>>,
    nextsn: Arc<RwLock<Box<HashMap<ReplicaId, SequenceNumber>>>>,
) -> Result<()> {
    info!("Starting pod view updater");
    // The task will be responsible for receiving vote messages from the vote_message channel and updating the pod view.
    while let Some(vote) = vote_message_receiver.recv().await {
        // Update
        // 5: upon receive 〈VOTE (tx, ts, sn, σ, Rj )〉 do ⊲ Received a vote from replica Rj
        // 16: if not Verify(pkj, (tx, ts, sn), σ) then return ⊲ Invalid vote
        // 17: if sn 6= nextsn[Rj ] then return ⊲ Vote cannot be processed yet
        // 18: nextsn[Rj ] ←nextsn[Rj ] + 1
        // 19: if ts < mrt[R[j]] then return ⊲ Rj sent old timestamp
        // 20: mrt[R[j]] ←ts
        // 21: if tx = heartBeat then return ⊲ Do not update tsps for heartBeat
        // 22: if tsps[tx][Rj ] 6= ⊥ and tsps[tx][Rj ] 6= ts then return ⊲ Duplicate timestamp
        // 23: tsps[tx][Rj ] ←ts
        // 24: end upon
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

        let mut nextsn = nextsn.write().await;
        let Some(nxtsn) = nextsn.get_mut(&replica_id) else {
            bail!("Bad initialization: Next sequence number not found for replica id: {}", &replica_id);
        };

        if sequence_number != *nxtsn {
            error!("Sequence number does not match next sequence number for replica id: {}. Vote ignored.", replica_id);
            warn!("This vote should go into the backlog (backlog is currently not implemented).");
            continue;
        }
        *nxtsn = *nxtsn + 1;
    

        let mut mrt = mrt.write().await;
        let Some(mrt) = mrt.get_mut(&replica_id) else {
            bail!("Bad initialization: MRT not found for replica id: {}", replica_id);
        };

        if timestamp < *mrt {
            error!("Replica sent old timestamp for replica id: {}. Vote ignored", replica_id);
            continue;
        }

        *mrt = timestamp;

        // if tx is HEARTBEAT skip updating tsps
        if !is_heartbeat(&transaction) {
            debug!("Updating timestamps for transaction: {:?}", transaction);
            let mut tsps = tsps.write().await;
            let Some(replica_timestamp_map) = tsps.get_mut(&transaction) else {
                bail!("Bad initialization: Timestamps not found for transaction: {:?}", &transaction);
            };

            if replica_timestamp_map.contains_key(&replica_id) || replica_timestamp_map[&replica_id] != timestamp {
                continue; // duplicated timestamp for the same transaction from the same replica
            }

            replica_timestamp_map.insert(replica_id, timestamp);
        } else {
            debug!("Received heartbeat from replica {}", replica_id);
        }
    };

    Ok(())
}

fn is_heartbeat(_tx: &Transaction) -> bool {
    info!("Checking if transaction is heartbeat");
    false
}

fn verify_vote_signature(_pk: &VerifyingKey, _transaction: &[u8], _timestamp: u64, _sequence_number: i64, _signature: &[u8]) -> bool {
    info!("Verifying vote signature");
    // Verify signature Verify(pk, (tx, ts, sn), sig)
    true
}


#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::VerifyingKey;
    use crate::EndpointInfo;
    
    #[tokio::test]
    async fn test_pod_network_client_init() {
        let mut pod_network_client = PodNetworkClient::new();
        let replicas = vec![
            Replica {
                id: 1,
                endpoint: EndpointInfo {
                    url: "ws://localhost:5002".to_string(),
                },
                pk: VerifyingKey::from_bytes(&[0; 32]).unwrap(),
            },
            Replica {
                id: 2,
                endpoint: EndpointInfo {
                    url: "ws://localhost:5003".to_string(),
                },
                pk: VerifyingKey::from_bytes(&[0; 32]).unwrap(),
            },
        ];
        let init_result = pod_network_client.init(replicas).await;
        assert!(init_result.is_ok());
    }
}
