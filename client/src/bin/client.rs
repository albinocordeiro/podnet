use std::fs::File;
use anyhow::{bail, Context, Result};
use clap::Parser;
use clientlib::pod_network_client::init_pod_network_client;
use clientlib::Replica;
use tracing::info;
use std::path::Path;

/// A simple client that reads replica configuration from a JSON file.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the replica configuration file in JSON format
    replicas_config_file: String,
    
    /// Alpha parameter for timestamp selection (number of timestamps to consider)
    #[arg(short, long, default_value_t = 1)]
    alpha: usize,
    
    /// Beta parameter for timestamp selection (number of padding values)
    #[arg(short, long, default_value_t = 0)]
    beta: usize,
    
    /// Port for the client API server
    #[arg(short, long, default_value_t = 50051)]
    port: u16,
}

/// Process the replica configuration file and return a vector of Replica objects
fn process_replica_config_file(path: &str) -> Result<Vec<Replica>> {
    let path = Path::new(path);
    let file = File::open(path)
        .with_context(|| format!("Failed to open replica config file: {}", path.display()))?;
    let contents = std::io::read_to_string(file)
        .with_context(|| format!("Failed to read replica config file: {}", path.display()))?;
    
    let replicas: Vec<Replica> = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse JSON in replica config file: {}", path.display()))?;

    Ok(replicas)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up tracing
    tracing_subscriber::fmt::init();
    info!("Starting client");

    let args = Args::parse();
    if args.alpha < 4 * args.beta + 1 {
        bail!("Alpha value is too low. Please choose a value greater than or equal to 4 * beta + 1.");
    }

    let replicas = process_replica_config_file(&args.replicas_config_file)
        .with_context(|| "Failed to process replica configuration file".to_string())?;
    if replicas.len() < args.alpha + args.beta {
        bail!("Insufficient replicas {} for given alpha {}, beta {} params", replicas.len(), args.alpha, args.beta);
    }
    
    init_pod_network_client(replicas, args.alpha, args.beta, args.port).await?;

    Ok(())
}