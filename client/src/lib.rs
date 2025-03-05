use ed25519_dalek::VerifyingKey;
use serde::{Serialize, Deserialize};
use url::Url;

pub mod clientapi {
    tonic::include_proto!("clientapi"); // The string must match the package name in your .proto file
}
pub mod client_api_service;
pub mod pod_network_client;
pub mod replica_comms_service;
pub mod pod_data_updater;

// Custom serialization module for hex encoding/decoding of VerifyingKey
pub mod hex_serde {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};
    
    pub fn serialize<S>(key: &VerifyingKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = key.as_bytes();
        let hex_string = hex::encode(bytes);
        serializer.serialize_str(&hex_string)
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<VerifyingKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        
        let hex_string = String::deserialize(deserializer)?;
        let bytes = hex::decode(&hex_string).map_err(|e| Error::custom(format!("Invalid hex string: {}", e)))?;
        
        let bytes_array: [u8; 32] = bytes.try_into()
            .map_err(|_| Error::custom("Invalid key length, must be 32 bytes"))?;
            
        VerifyingKey::from_bytes(&bytes_array)
            .map_err(|e| Error::custom(format!("Invalid ed25519 public key: {}", e)))
            
    }
}

pub type ReplicaId = u64;
pub type Timestamp = u64;
pub type SequenceNumber = u64;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Replica {
    pub id: ReplicaId,
    pub url: Url,
    #[serde(with = "hex_serde")]
    pub pk: VerifyingKey,
}

impl Replica {
    pub fn new(id: ReplicaId, url: Url, pk: VerifyingKey) -> Replica {
        Replica { id, url, pk }
    }
}
