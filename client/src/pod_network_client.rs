use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::info;

use crate::client_api_service::start_api_server;
use crate::replica_comms_service::start_replica_comms_service;
use crate::pod_data_updater::start_pod_data_updater;

use crate::{
    Replica, ReplicaId, SequenceNumber, Timestamp,
};
use crate::clientapi::{Transaction, Vote};
pub const CHANNEL_SIZE: usize = 100;

/// Initializes the pod network client with the given replicas list and configuration parameters.
pub async fn init_pod_network_client(replicas_list: Vec<Replica>, alpha: usize, beta: usize, port: u16) -> Result<()> {
    info!("Initializing the pod network client");
    
    let mut mrt = HashMap::new();
    let tsps = HashMap::new();
    let mut nextsn = HashMap::new();
    let ts_vote_msg = HashMap::new();

    for replica in replicas_list.iter().cloned() {
        mrt.insert(replica.id, 0);
        nextsn.insert(replica.id, 0);
    }
    let replicas = Arc::new(RwLock::new(Box::new(replicas_list.iter().cloned().map(|replica| (replica.id, replica)).collect())));
    let mrt = Arc::new(RwLock::new(Box::new(mrt)));
    let tsps = Arc::new(RwLock::new(Box::new(tsps)));
    let nextsn = Arc::new(RwLock::new(Box::new(nextsn)));
    let ts_vote_msg = Arc::new(RwLock::new(Box::new(ts_vote_msg)));

    let (write_request_sender, write_request_receiver) = broadcast::channel(CHANNEL_SIZE);
    let (vote_message_sender, vote_message_receiver) = mpsc::channel(CHANNEL_SIZE);

    let api_server = start_api_server(
        write_request_sender, 
        replicas.clone(),
        tsps.clone(),
        mrt.clone(),
        nextsn.clone(),
        ts_vote_msg.clone(),
        alpha,
        beta,
        port,
    );

    let pod_view_updater = start_pod_data_updater(
        vote_message_receiver,
        replicas.clone(),
        tsps.clone(), 
        mrt.clone(),
        nextsn.clone(),
        ts_vote_msg.clone(),
    );

    let replica_comms = start_replica_comms_service(
        replicas_list, 
        write_request_receiver, 
        vote_message_sender
    );

    tokio::select! {
        result = api_server => {
            info!("API server quit with result: {:?}", result);
            result
        },
        result = pod_view_updater => {
            info!("View updater quit with result: {:?}", result);
            result
        },
        result = replica_comms => {
            info!("WebSocket supervisor quit with result: {:?}", result);
            result
        }
    }
}
