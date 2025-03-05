use podlib::pod_network_node::init_pod_network_node;
use anyhow::{Context, Result};
use clap::Parser;
use tracing::info;
use ed25519_dalek::SigningKey;
use hex;

/// A simple replica node for the Pod Network
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Replica ID
    #[arg(short, long, default_value_t = 0)]
    replica_id: u64,

    /// Path to the replica log file
    #[arg(short = 'f', long, default_value = "replica_wal")]
    replica_log_file: String,
    
    /// Port to listen on
    #[arg(short, long, default_value_t = 12345)]
    port: u16,

    /// Private key in hex format
    #[arg(short = 'k', long, default_value = "e4f6cfdbd7503b2c27b8272362aa7200e92d82154e69f41402badb968e836e90")]
    private_key: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up tracing
    tracing_subscriber::fmt::init();

    info!("Initializing replica node");
    
    // Parse command line arguments
    let args = Args::parse();
    
    // Parse the private key
    let private_key_bytes = hex::decode(&args.private_key)
        .context("Failed to decode private key hex")?;
    
    // Create SigningKey from bytes
    if private_key_bytes.len() != 32 {
        anyhow::bail!("Private key must be exactly 32 bytes");
    }
    
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&private_key_bytes);
    let signing_key = SigningKey::from_bytes(&key_bytes);
    
    // Check if the replica log file exists and log the information
    if std::path::Path::new(&args.replica_log_file).exists() {
        info!("Using existing replica log file: {}", args.replica_log_file);
    } else {
        info!("Creating new replica log file: {}", args.replica_log_file);
    }

    info!("Starting replica with ID: {}, listening on port: {}", args.replica_id, args.port);
    
    // Initialize the pod network node
    init_pod_network_node(
        args.replica_id,
        args.port,
        &args.replica_log_file,
        signing_key
    ).await?;

info!("Replica node exited without errors.");

    Ok(())
}