pub mod replica_log;
pub mod pod_network_node;
pub mod client_comms_service;
pub mod log_vote_service;
pub mod heartbeat_service;
pub mod utils;
// Re-export the generated code
pub mod replicaapi {
    tonic::include_proto!("replicaapi");
}

pub type ClientId = u64;
pub const ROUND_SIZE_MS: u64 = 100;