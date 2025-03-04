use tonic::{Request, Response, Status, transport::Server};

use crate::clientapi::client_api_server::{ClientApi, ClientApiServer};
use crate::clientapi::{PodData, ReadMessage, ReadReply, WriteMessage, WriteReply, Vote, TxRecord, Transaction};
use tracing::info;

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

use anyhow::Result;

use crate::{Replica, ReplicaId, SequenceNumber, Timestamp};

const BETA: usize = 0;

#[derive(Debug)]
pub struct ClientApiService {
    pub write_request_sender: tokio::sync::broadcast::Sender<Transaction>,
    pub replicas: Arc<RwLock<Box<HashMap<ReplicaId, Replica>>>>,
    pub tsps: Arc<RwLock<Box<HashMap<Transaction, HashMap<ReplicaId, Timestamp>>>>>,
    pub mrt: Arc<RwLock<Box<HashMap<ReplicaId, Timestamp>>>>,
    pub nextsn: Arc<RwLock<Box<HashMap<ReplicaId, SequenceNumber>>>>,
    pub ts_vote_msg: Arc<RwLock<Box<HashMap<Timestamp, Vote>>>>,
}

impl ClientApiService {
    pub async fn min_possible_ts(&self, tx: &Transaction) -> Timestamp {
        // 1: function minPossibleTs( tx)
        // 2: timestamps ←[ ]
        // 3: for Rj ∈R do
        // 4: if tsps[tx][Rj ] 6= ⊥ then
        // 5: timestamps ←timestamps ‖[tsps[tx][Rj ]]
        // 6: else
        // 7: timestamps ←timestamps ‖[mrt[Rj ]]
        // 8: end if
        // 9: end for
        // 10: sort timestamps in increasing order
        // 11: timestamps ←[0, β times. . . , 0] ‖timestamps
        // 12: return median(timestamps[: α])
        // 13: end function
        let mut timestamps: Vec<Timestamp> = Vec::new();
        let replicas = self.replicas.read().await;
        let alpha = replicas.len();
        let beta = BETA;
        let tsps = self.tsps.read().await;
        let mrt = self.mrt.read().await;
        for replica_id in replicas.keys() {
            if let Some(tx_timestamps) = tsps.get(tx) {
                if let Some(timestamp) = tx_timestamps.get(replica_id) {
                    timestamps.push(*timestamp);
                } 
            } else if let Some(mrt_timestamp) = mrt.get(replica_id) {
                timestamps.push(*mrt_timestamp);
            }
        }
        timestamps.sort();
        let mut first_part = vec![0; beta];
        first_part.extend(timestamps.iter().cloned());
        let timestamps = first_part.iter().take(alpha).cloned().collect::<Vec<_>>();
        // return the median of timestamps
        timestamps.get(timestamps.len() / 2).cloned().unwrap_or_default()
    }

    pub async fn min_possible_ts_for_new_tx(&self) -> Timestamp {
        // 27: function minPossibleTsForNewTx()
        // 28: timestamps ←mrt
        // 29: sort timestamps in increasing order
        // 30: timestamps ←[0, β times. . . , 0] ‖timestamps
        // 31: return median(timestamps[: α])
        // 32: end function
        let replicas = self.replicas.read().await;
        let alpha = replicas.len();
        let beta = BETA;
        let mrt = self.mrt.read().await;
        let mut timestamps = vec![0; beta];
        timestamps.extend(mrt.iter().map(|t|t.1.clone()));
        timestamps.sort();
        let timestamps = timestamps.iter().take(alpha).cloned().collect::<Vec<_>>();
        // return the median of timestamps
        timestamps.get(timestamps.len() / 2).cloned().unwrap_or_default()
    }

    pub async fn max_possible_ts(&self, tx: &Transaction) -> Timestamp {
        // 14: function maxPossibleTs( tx)
        // 15: timestamps ←[ ]
        // 16: for Rj ∈R do
        // 17: if tsps[tx][Rj ] 6= ⊥ then
        // 18: timestamps ←timestamps ‖[tsps[tx][Rj ]]
        // 19: else
        // 20: timestamps ←timestamps ‖[∞]
        // 21: end if
        // 22: end for
        // 23: sort timestamps in increasing order
        // 24: timestamps ←timestamps ‖[∞, β times. . . , ∞]
        // 25: return median(timestamps[−α :])
        // 26: end function
        let mut timestamps: Vec<Timestamp> = Vec::new();
        let replicas = self.replicas.read().await;
        let alpha = replicas.len();
        let beta = BETA;

        let tsps = self.tsps.read().await;
        for replica_id in replicas.keys() {
            if let Some(tx_timestamps) = tsps.get(tx) {
                if let Some(timestamp) = tx_timestamps.get(replica_id) {
                    timestamps.push(*timestamp);
                } 
            } else {
                timestamps.push(u64::MAX);
            }
        }
        timestamps.sort();
        timestamps.extend(std::iter::repeat(u64::MAX).take(beta));
        let timestamps = timestamps.split_off(timestamps.len() - alpha);
        

        // return the median of timestamps
        timestamps.get(timestamps.len() / 2).cloned().unwrap_or_default()
    }
}

#[tonic::async_trait]
impl ClientApi for ClientApiService {

    async fn write(&self, request: Request<WriteMessage>) -> Result<Response<WriteReply>, Status> {
        let transaction = request
            .into_inner()
            .transaction
            .ok_or_else(||Status::internal("No transaction data in request"))?;

        // Send the transaction to the replica broadcast channel
        self.write_request_sender
            .send(transaction)
            .map_err(|e| Status::internal(format!("Failed to process write request: {}", e)))?;

        Ok(Response::new(WriteReply {
            success: true,
        }))
    }

    async fn read(&self, _request: Request<ReadMessage>) -> Result<Response<ReadReply>, Status> {
        // Implements the Algorithm 2 of the pod-core.pdf
        // 6: function read()
        // 7: T ←∅; Cpp ←[ ]
        // 8: for tx ∈tsps.keys() do ⊲ Loop over all received transactions
        // 9: rmin ←minPossibleTs(tx)
        // 10: rmax ←maxPossibleTs(tx)
        // 11: rconf ←⊥; Ctx ←[ ]; timestamps = [ ]
        // 12: if |tsps[tx].values()|≥α then
        // 13: for Rj ∈tsps[tx].values() do
        // 14: ts ←tsps[tx][Rj ]
        // 15: timestamps ←timestamps ‖ts
        // 16: Ctx ←Ctx ‖ts.getVoteMsg()
        // 17: end for
        // 18: rconf ←median(timestamps)
        // 19: end if
        // 20: T ←T ∪{(tx, rmin, rmax, rconf, Ctx)}
        // 21: end for
        // 22: rperf ←minPossibleTsForNewTx()
        // 23: for Rj ∈R do
        // 24: Cpp ←Cpp ‖mrt[Rj ].getVoteMsg()
        // 25: end for
        // 26: D ←(T, rperf, Cpp)
        // 27: return D
        // 28: end function
        let mut t = Vec::new();
        let tsps = self.tsps.read().await;
        let ts_vote_msg = self.ts_vote_msg.read().await;
        for tx in tsps.keys() {
            let rmin = self.min_possible_ts(tx).await;
            let rmax = self.max_possible_ts(tx).await;
            let mut rconf = None;
            let mut ctx = Vec::new();
            let mut timestamps = Vec::new();
            let tspstx = tsps.get(tx);
            if let Some(tspstx) = tspstx {
                for replica_id in tspstx.keys() {
                    let Some(ts) = tspstx.get(replica_id) else {
                        continue;
                    };
                    timestamps.push(*ts);
                    ctx.push(ts_vote_msg.get(ts).cloned().unwrap_or_default());
                }
                rconf = Some(timestamps.get(timestamps.len() / 2).cloned().unwrap_or_default());
            }
            t.push(TxRecord {
                transaction: Some(tx.clone()),
                rmin,
                rmax,
                rconf,
                ctx,
            });
        }
        let rperf = self.min_possible_ts_for_new_tx().await;
        let mrt = self.mrt.read().await;
        
        let cpp: Vec<Vote> = mrt.iter().map(|t| ts_vote_msg[t.1].clone()).collect();

        let pod_data = PodData { t, rperf, cpp };

        Ok(Response::new(ReadReply {
            pod_data: Some(pod_data),
        }))
    }
}

pub async fn start_api_server(
    write_request_sender: tokio::sync::broadcast::Sender<Transaction>,
    replicas: Arc<RwLock<Box<HashMap<ReplicaId, Replica>>>>,
    tsps: Arc<RwLock<Box<HashMap<Transaction, HashMap<ReplicaId, Timestamp>>>>>,
    mrt: Arc<RwLock<Box<HashMap<ReplicaId, Timestamp>>>>,
    nextsn: Arc<RwLock<Box<HashMap<ReplicaId, SequenceNumber>>>>,
    ts_vote_msg: Arc<RwLock<Box<HashMap<Timestamp, Vote>>>>, 
) -> Result<()> {
    // Start the RPC API server
    let addr = "[::1]:50051".parse()?;

    info!("Starting RPC API server on {}", addr);

    let service = ClientApiService {
        write_request_sender,
        replicas,
        mrt,
        nextsn,
        tsps, 
        ts_vote_msg,
    };

    Server::builder()
        .add_service(ClientApiServer::new(service))
        .serve(addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start RPC server: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::runtime::Runtime;
    use tokio::sync::RwLock;
    use ed25519_dalek::VerifyingKey;
    use crate::EndpointInfo;
    
    #[test]
    fn test_min_possible_ts() {
        // Create a mock runtime for async testing
        let rt = Runtime::new().unwrap();

        // Test 1: Empty system (no replicas, no timestamps)
        rt.block_on(async {
            let service = ClientApiService {
                replicas: Arc::new(RwLock::new(Box::new(HashMap::new()))),
                mrt: Arc::new(RwLock::new(Box::new(HashMap::new()))),
                tsps: Arc::new(RwLock::new(Box::new(HashMap::new()))),
                ts_vote_msg: Arc::new(RwLock::new(Box::new(HashMap::new()))),
                write_request_sender: tokio::sync::broadcast::channel(10).0,
                nextsn: Arc::new(RwLock::new(Box::new(HashMap::new()))),
            };

            let tx = Transaction { data: vec![1, 2, 3] };
            let ts = service.min_possible_ts(&tx).await;
            assert_eq!(ts, 0, "Empty system should return 0");
        });

        // Test 2: System with replicas but no timestamps for the transaction
        rt.block_on(async {
            let mut replicas = HashMap::new();
            let mut mrt = HashMap::new();
            
            // Create dummy VerifyingKey for tests
            let dummy_key = [0u8; 32];
            let dummy_vk = VerifyingKey::from_bytes(&dummy_key).unwrap_or_else(|_| panic!("Failed to create dummy key"));
            
            // Add three replicas with different MRT values
            replicas.insert(1, Replica { id: 1, endpoint: EndpointInfo { url: "url1".to_string() }, pk: dummy_vk });
            replicas.insert(2, Replica { id: 2, endpoint: EndpointInfo { url: "url2".to_string() }, pk: dummy_vk });
            replicas.insert(3, Replica { id: 3, endpoint: EndpointInfo { url: "url3".to_string() }, pk: dummy_vk });
            
            mrt.insert(1, 10);
            mrt.insert(2, 20);
            mrt.insert(3, 30);

            let service = ClientApiService {
                replicas: Arc::new(RwLock::new(Box::new(replicas))),
                mrt: Arc::new(RwLock::new(Box::new(mrt))),
                tsps: Arc::new(RwLock::new(Box::new(HashMap::new()))),
                ts_vote_msg: Arc::new(RwLock::new(Box::new(HashMap::new()))),
                write_request_sender: tokio::sync::broadcast::channel(10).0,
                nextsn: Arc::new(RwLock::new(Box::new(HashMap::new()))),
            };

            let tx = Transaction { data: vec![1, 2, 3] };
            let ts = service.min_possible_ts(&tx).await;
            
            // Since BETA = 0 and we're using MRT values [10, 20, 30],
            // the median should be 20
            assert_eq!(ts, 20, "Should return median of MRT values");
        });

        // Test 3: System with replicas and timestamps for the transaction
        rt.block_on(async {
            let mut replicas = HashMap::new();
            let mut mrt = HashMap::new();
            let mut tsps: HashMap<Transaction, HashMap<ReplicaId, Timestamp>> = HashMap::new();
            
            // Create dummy VerifyingKey for tests
            let dummy_key = [0u8; 32];
            let dummy_vk = VerifyingKey::from_bytes(&dummy_key).unwrap_or_else(|_| panic!("Failed to create dummy key"));
            
            // Add three replicas
            replicas.insert(1, Replica { id: 1, endpoint: EndpointInfo { url: "url1".to_string() }, pk: dummy_vk });
            replicas.insert(2, Replica { id: 2, endpoint: EndpointInfo { url: "url2".to_string() }, pk: dummy_vk });
            replicas.insert(3, Replica { id: 3, endpoint: EndpointInfo { url: "url3".to_string() }, pk: dummy_vk });
            
            // Add MRT values (these should be ignored if tsps has values)
            mrt.insert(1, 100);
            mrt.insert(2, 200);
            mrt.insert(3, 300);

            // Create a transaction and add timestamps for it
            let tx = Transaction { data: vec![1, 2, 3] };
            let mut tx_timestamps = HashMap::new();
            tx_timestamps.insert(1, 5);
            tx_timestamps.insert(2, 15);
            tx_timestamps.insert(3, 25);

            let mut all_tsps = HashMap::new();
            all_tsps.insert(tx.clone(), tx_timestamps);

            let service = ClientApiService {
                replicas: Arc::new(RwLock::new(Box::new(replicas))),
                mrt: Arc::new(RwLock::new(Box::new(mrt))),
                tsps: Arc::new(RwLock::new(Box::new(all_tsps))),
                ts_vote_msg: Arc::new(RwLock::new(Box::new(HashMap::new()))),
                write_request_sender: tokio::sync::broadcast::channel(10).0,
                nextsn: Arc::new(RwLock::new(Box::new(HashMap::new()))),
            };

            let ts = service.min_possible_ts(&tx).await;
            
            // Since BETA = 0 and we're using timestamp values [5, 15, 25],
            // the median should be 15
            assert_eq!(ts, 15, "Should return median of transaction timestamps");
        });

        // Test 4: System with replicas and mixed sources (some from tsps, some from mrt)
        rt.block_on(async {
            let mut replicas = HashMap::new();
            let mut mrt = HashMap::new();
            let mut tsps: HashMap<Transaction, HashMap<ReplicaId, Timestamp>> = HashMap::new();
            
            // Create dummy VerifyingKey for tests
            let dummy_key = [0u8; 32];
            let dummy_vk = VerifyingKey::from_bytes(&dummy_key).unwrap_or_else(|_| panic!("Failed to create dummy key"));
            
            // Add three replicas
            replicas.insert(1, Replica { id: 1, endpoint: EndpointInfo { url: "url1".to_string() }, pk: dummy_vk });
            replicas.insert(2, Replica { id: 2, endpoint: EndpointInfo { url: "url2".to_string() }, pk: dummy_vk });
            replicas.insert(3, Replica { id: 3, endpoint: EndpointInfo { url: "url3".to_string() }, pk: dummy_vk });
            
            // Create a transaction and add timestamps for only some replicas
            let tx = Transaction { data: vec![1, 2, 3] };
            let mut tx_timestamps = HashMap::new();
            tx_timestamps.insert(1, 5);  // Use tsps for replica 1
            // No entry for replica 2, so it will use mrt
            tx_timestamps.insert(3, 25); // Use tsps for replica 3

            let mut all_tsps = HashMap::new();
            all_tsps.insert(tx.clone(), tx_timestamps);

            let service = ClientApiService {
                replicas: Arc::new(RwLock::new(Box::new(replicas))),
                mrt: Arc::new(RwLock::new(Box::new(mrt))),
                tsps: Arc::new(RwLock::new(Box::new(all_tsps))),
                ts_vote_msg: Arc::new(RwLock::new(Box::new(HashMap::new()))),
                write_request_sender: tokio::sync::broadcast::channel(10).0,
                nextsn: Arc::new(RwLock::new(Box::new(HashMap::new()))),
            };

            let ts = service.min_possible_ts(&tx).await;
            
            // Values should be [5, 200, 25], sorted as [5, 25, 200]
            // Since BETA = 0, the median is 25
            assert_eq!(ts, 25, "Should return median of mixed sources");
        });
    }
}
