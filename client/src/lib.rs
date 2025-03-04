use ed25519_dalek::VerifyingKey;

pub mod clientapi {
    tonic::include_proto!("clientapi"); // The string must match the package name in your .proto file
}
pub mod client_api_service;
pub mod pod_network_client;
pub mod replica_comm_service;

pub type ReplicaId = u64;
pub type Timestamp = u64;
pub type SequenceNumber = i64;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EndpointInfo {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Replica {
    pub id: ReplicaId,
    pub endpoint: EndpointInfo,
    pub pk: VerifyingKey,
}

impl Replica {
    pub fn new(id: ReplicaId, endpoint: EndpointInfo, pk: VerifyingKey) -> Replica {
        Replica { id, endpoint, pk }
    }
}
