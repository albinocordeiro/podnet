use tonic::{Request, Response, Status, transport::Server};

use crate::clientapi::client_api_server::{ClientApi, ClientApiServer};
use crate::clientapi::{PodData, ReadMessage, ReadReply, WriteMessage, WriteReply, Vote, TxRecord, Transaction};
use tracing::info;

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

use anyhow::Result;

use crate::{Replica, ReplicaId, SequenceNumber, Timestamp};

#[derive(Debug)]
pub struct ClientApiService {
    pub write_request_sender: tokio::sync::broadcast::Sender<Transaction>,
    pub replicas: Arc<RwLock<Box<HashMap<ReplicaId, Replica>>>>,
    pub tsps: Arc<RwLock<Box<HashMap<Transaction, HashMap<ReplicaId, Timestamp>>>>>,
    pub mrt: Arc<RwLock<Box<HashMap<ReplicaId, Timestamp>>>>,
    pub nextsn: Arc<RwLock<Box<HashMap<ReplicaId, SequenceNumber>>>>,
    pub ts_vote_msg: Arc<RwLock<Box<HashMap<Timestamp, Vote>>>>,
    pub alpha: usize,
    pub beta: usize,
}

impl ClientApiService {
    pub async fn min_possible_ts(&self, tx: &Transaction) -> Timestamp {
        let mut timestamps: Vec<Timestamp> = Vec::new();
        let replicas = self.replicas.read().await;
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
        let mut first_part = vec![0; self.beta];
        first_part.extend(timestamps.iter().cloned());
        let timestamps = first_part.iter().take(self.alpha).cloned().collect::<Vec<_>>();
        // return the median of timestamps
        timestamps.get(timestamps.len() / 2).cloned().unwrap_or_default()
    }

    pub async fn min_possible_ts_for_new_tx(&self) -> Timestamp {
        let mrt = self.mrt.read().await;
        let mut timestamps = vec![0; self.beta];
        timestamps.extend(mrt.iter().map(|t|t.1.clone()));
        timestamps.sort();
        let timestamps = timestamps.iter().take(self.alpha).cloned().collect::<Vec<_>>();
        // return the median of timestamps
        timestamps.get(timestamps.len() / 2).cloned().unwrap_or_default()
    }

    pub async fn max_possible_ts(&self, tx: &Transaction) -> Timestamp {
        let mut timestamps: Vec<Timestamp> = Vec::new();
        let replicas = self.replicas.read().await;
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
        timestamps.extend(std::iter::repeat(u64::MAX).take(self.beta));
        let timestamps = timestamps.split_off(timestamps.len() - self.alpha);
        

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
        
        let cpp: Vec<Vote> = mrt.iter().map(|t| ts_vote_msg.get(t.1).cloned().unwrap_or_default()).collect();

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
    alpha: usize,
    beta: usize,
    port: u16,
) -> Result<()> {
    // Start the RPC API server
    let addr = format!("[::1]:{}", port).parse()?;

    info!("Starting RPC API server on {}", addr);

    let service = ClientApiService {
        write_request_sender,
        replicas,
        mrt,
        nextsn,
        tsps, 
        ts_vote_msg,
        alpha,
        beta,
    };

    Server::builder()
        .add_service(ClientApiServer::new(service))
        .serve(addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start RPC server: {}", e))?;

    Ok(())
}
