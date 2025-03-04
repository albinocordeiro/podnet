use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read}; 

use anyhow::{Context, Result};
use clap::Parser;
use client::pod_network_client::PodNetworkClient;
use client::{EndpointInfo, Replica};
use ed25519_dalek::VerifyingKey;
use tracing::info;

/// A simple client that reads replica configuration files.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the replica public key file.
    replica_pk_file: String,

    /// Path to the replica endpoints file.
    replica_endpoints_file: String,
}

// This function reads a replica public key file and returns a map from replica ID to public key, playing the role of the PKI.
fn process_replica_pk_file(path: &str) -> Result<HashMap<u64, VerifyingKey>> {
    let file = File::open(path).with_context(|| format!("Failed to open file: {}", path))?;
    let reader = BufReader::new(file);
    let mut map = HashMap::new();

    for (i, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("Failed to read line {}.", i + 1))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let id_str = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Missing information on line {}.", i + 1))?;
        let key = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Missing information on line {}.", i + 1))?
            .to_string();
        let bytes = hex::decode(&key)
            .with_context(|| format!("Invalid hex string on line {}.", i + 1))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid length on line {}.", i + 1))?;

        let pk = VerifyingKey::from_bytes(&bytes)
            .with_context(|| format!("Invalid ed25519 public key on line {}.", i + 1))?;

        let id = id_str
            .parse()
            .with_context(|| format!("Invalid replica ID '{}' on line {}.", id_str, i + 1))?;
        map.insert(id, pk);
    }

    Ok(map)
}

fn process_replica_endpoints_file(path: &str) -> Result<HashMap<u64, EndpointInfo>> {
    let file = File::open(path).with_context(|| format!("Failed to open file: {}", path))?;
    let reader = BufReader::new(file);
    let mut endpoints = HashMap::new();

    for (i, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("Failed to read line {}.", i + 1))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let id_str = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Missing Replica ID on line {}.", i + 1))?;

        let id = id_str
            .parse()
            .with_context(|| format!("Invalid replica ID '{}' on line {}.", id_str, i + 1))?;

        let endpoint_str = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Missing endpoint information on line {}.", i + 1))?;

        let info = EndpointInfo {
            url: endpoint_str.to_string(),
        };
        {
            let parsed = url::Url::parse(&info.url)
                .with_context(|| format!("Invalid URL format on line {}.", i + 1))?;
            let scheme = parsed.scheme();
            if scheme != "ws" && scheme != "wss" {
                anyhow::bail!("Invalid websocket URL scheme on line {}.", i + 1);
            }
        }
        endpoints.insert(id, info);
    }
    Ok(endpoints)
}

#[tokio::main]
async fn main() -> Result<()> {
    info!("Starting client");

    let args = Args::parse();

    let replica_pks = process_replica_pk_file(&args.replica_pk_file)
        .with_context(|| "Failed to process replica->public key file".to_string())?;

    let replica_endpoints = process_replica_endpoints_file(&args.replica_endpoints_file)
        .with_context(|| "Failed to process replica->endpoints file".to_string())?;

    let replicas = replica_pks
        .keys()
        .cloned()
        .into_iter()
        .map(|id| {
            let pk = replica_pks.get(&id).expect("Expected replica ID");
            let endpoint = replica_endpoints.get(&id).expect("Expected replica ID");
            Replica {
                id,
                pk: pk.clone(),
                endpoint: endpoint.clone(),
            }
        })
        .collect::<Vec<Replica>>();

    let mut pod_network_client = PodNetworkClient::new();

    pod_network_client.init(replicas).await?;

    Ok(())
}

#[cfg(test)]
mod pks_tests {
    use tempfile::NamedTempFile;
    use std::io::Write;
    use super::*;

    // A known valid ed25519 public key (base point) in hex.
    const VALID_PUBKEY_HEX: &str =
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

    // Helper to write content to a temporary file and return its path.
    fn write_temp_file(contents: &str) -> (NamedTempFile, String) {
        let mut file = tempfile::NamedTempFile::with_prefix_in("replica_pk_", "/tmp")
            .expect("Failed to create temp file");
        write!(file, "{}", contents).expect("Failed to write to temp file");
        let file_path = file.path().to_string_lossy().into_owned();
        (file, file_path)
    }

    #[test]
    fn test_valid_input() {
        // Create a file with one valid replica entry.
        let contents = format!("1 {}\n", VALID_PUBKEY_HEX);
        let path = write_temp_file(&contents);
        println!("Temporary file path: {}", path.1);
        let map = process_replica_pk_file(&path.1)
            .expect(format!("Processing valid file failed at {}", path.1).as_str());

        // Parse expected public key.
        let expected_bytes = hex::decode(VALID_PUBKEY_HEX).unwrap().try_into().unwrap();
        let expected_pk = VerifyingKey::from_bytes(&expected_bytes).unwrap();

        assert_eq!(map.len(), 1);
        let pk = map.get(&1).expect("Expected replica id 1");
        assert_eq!(pk.as_bytes(), expected_pk.as_bytes());
    }

    #[test]
    fn test_invalid_hex() {
        // Line with an invalid hex string.
        let contents = "1 nothex\n";
        let path = write_temp_file(contents);

        let result = process_replica_pk_file(&path.1);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid hex string"));
    }

    #[test]
    fn test_invalid_length() {
        // Create a hex string with valid hex but incorrect length (e.g., 40 hex digits instead of 64).
        let invalid_length_hex = "0123456789abcdef0123456789abcdef01234567"; // 20 bytes
        let contents = format!("1 {}\n", invalid_length_hex);
        let path = write_temp_file(&contents);

        let result = process_replica_pk_file(&path.1);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid length"));
    }

    #[test]
    fn test_missing_replica_id() {
        // Missing replica id.
        let contents = format!(" {}\n", VALID_PUBKEY_HEX);
        let path = write_temp_file(&contents);

        let result = process_replica_pk_file(&path.1);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Missing information"));
    }

    #[test]
    fn test_invalid_replica_id() {
        // Replica id is non-numeric.
        let contents = format!("abc {}\n", VALID_PUBKEY_HEX);
        let path = write_temp_file(&contents);

        let result = process_replica_pk_file(&path.1);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid replica ID"));
    }

    #[test]
    fn test_missing_public_key() {
        // Missing public key.
        let contents = "1\n";
        let path = write_temp_file(contents);

        let result = process_replica_pk_file(&path.1);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Missing information"));
    }
}

#[cfg(test)]
mod endpoints_tests {
    use tempfile::NamedTempFile;
    use std::io::Write;
    use super::*;

    // Helper to write content to a temporary file and return its path.
    fn write_temp_file(contents: &str) -> (NamedTempFile, String) {
        let mut file = NamedTempFile::with_prefix_in("replica_endpoint_", "/tmp")
            .expect("Failed to create temp file");
        write!(file, "{}", contents).expect("Failed to write to temp file");
        let file_path = file.path().to_string_lossy().into_owned();
        (file, file_path)
    }

    #[test]
    fn test_valid_endpoint_input() {
        // Create a file with one valid replica endpoint entry.
        let valid_url = "ws://localhost:8080";
        let contents = format!("1 {}\n", valid_url);
        let (_temp_file, file_path) = write_temp_file(&contents);

        let endpoints = process_replica_endpoints_file(&file_path)
            .expect("Should successfully parse a valid endpoints file");
        assert_eq!(endpoints.len(), 1);
        let info = endpoints.get(&1).expect("Expected replica id 1");
        assert_eq!(info.url, valid_url);
    }

    #[test]
    fn test_invalid_url_format() {
        // Invalid URL string that doesn't parse.
        let invalid_url = "not_a_valid_url";
        let contents = format!("1 {}\n", invalid_url);
        let (_temp_file, file_path) = write_temp_file(&contents);

        let result = process_replica_endpoints_file(&file_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        println!("{}", err);
        assert!(err.contains("Invalid URL format"));
    }

    #[test]
    fn test_invalid_scheme() {
        // Valid URL format but invalid scheme (not ws or wss).
        let invalid_url = "http://localhost:8080";
        let contents = format!("1 {}\n", invalid_url);
        let (_temp_file, file_path) = write_temp_file(&contents);

        let result = process_replica_endpoints_file(&file_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid websocket URL scheme"));
    }

    #[test]
    fn test_missing_replica_id() {
        // Missing replica id.
        let valid_url = "ws://localhost:8080";
        let contents = format!(" {}\n", valid_url);
        let (_temp_file, file_path) = write_temp_file(&contents);

        let result = process_replica_endpoints_file(&file_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        println!("{}", err);
        assert!(err.contains("Invalid replica ID"));
    }

    #[test]
    fn test_invalid_replica_id() {
        // Replica id is non-numeric.
        let valid_url = "ws://localhost:8080";
        let contents = format!("abc {}\n", valid_url);
        let (_temp_file, file_path) = write_temp_file(&contents);

        let result = process_replica_endpoints_file(&file_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid replica ID"));
    }

    #[test]
    fn test_missing_endpoint() {
        // Missing endpoint information.
        let contents = "1\n";
        let (_temp_file, file_path) = write_temp_file(contents);

        let result = process_replica_endpoints_file(&file_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Missing endpoint information"));
    }
}
