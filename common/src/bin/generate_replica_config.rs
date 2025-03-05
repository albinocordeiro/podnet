use clap::Parser;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use url::Url;

/// Generate configuration files for replicas with valid Ed25519 key pairs
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Number of replicas to generate
    #[arg(short, long, default_value_t = 3)]
    count: usize,
    
    /// Base URL for replicas (port will be incremented for each replica)
    #[arg(short, long, default_value = "ws://localhost:12345")]
    base_url: String,
    
    /// Output path for the replicas_config.json file
    #[arg(short, long, default_value = "replicas_config.json")]
    output: PathBuf,
    
    /// Output path for the private keys file
    #[arg(short = 'k', long, default_value = "private_keys.json")]
    privkey_output: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
struct ReplicaConfig {
    id: u64,
    url: String,
    pk: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct PrivateKeyConfig {
    id: u64,
    port: u16,
    privk: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    // Validate base URL
    let base_url = Url::parse(&args.base_url)?;
    if base_url.scheme() != "ws" && base_url.scheme() != "wss" {
        eprintln!("Warning: URL scheme is not ws or wss, which may not be valid for WebSocket connections");
    }
    
    // Extract host and port from base URL
    let host = base_url.host_str().unwrap_or("localhost");
    let base_port = base_url.port().unwrap_or(80);
    let scheme = base_url.scheme();
    
    let mut replica_configs = Vec::new();
    let mut private_key_configs = Vec::new();
    
    // Generate key pairs for each replica
    for i in 0..args.count {
        // Create a new SigningKey
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        
        // Get the public key
        let verifying_key = signing_key.verifying_key();
        
        // Construct URL with incrementing port
        let port = base_port + i as u16;
        let url = format!("{}://{}:{}", scheme, host, port);
        
        // Public key as hex
        let public_key_hex = hex::encode(verifying_key.as_bytes());
        
        // Private key as hex (note: in a real app, this should be securely stored)
        let private_key_hex = hex::encode(signing_key.to_bytes());
        
        // Add to configurations
        replica_configs.push(ReplicaConfig {
            id: i as u64,
            url,
            pk: public_key_hex,
        });
        
        private_key_configs.push(PrivateKeyConfig {
            id: i as u64,
            port,
            privk: private_key_hex,
        });
    }
    
    // Write public configurations to file
    let replica_json = serde_json::to_string_pretty(&replica_configs)?;
    let mut file = File::create(&args.output)?;
    file.write_all(replica_json.as_bytes())?;
    println!("Wrote replica configurations to {}", args.output.display());
    
    // Write private keys to file
    let privkey_json = serde_json::to_string_pretty(&private_key_configs)?;
    let mut file = File::create(&args.privkey_output)?;
    file.write_all(privkey_json.as_bytes())?;
    println!("Wrote private keys to {}", args.privkey_output.display());
    
    Ok(())
}