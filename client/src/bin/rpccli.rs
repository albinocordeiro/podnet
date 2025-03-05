use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use clientlib::clientapi::client_api_client::ClientApiClient;
use clientlib::clientapi::{ReadMessage, Transaction, WriteMessage};
use tonic::transport::Channel;
use tracing::info;

/// A CLI tool for interacting with the Pod Network Client API
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Port for the client API server
    #[arg(short, long, default_value_t = 50051)]
    port: u16,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Read current state from the Pod Network
    Read,
    
    /// Send transaction data to the Pod Network
    Send {
        /// Transaction data as a string
        transaction_data: String,
    },
}

async fn create_client(port: u16) -> Result<ClientApiClient<Channel>> {
    let addr = format!("http://[::1]:{}", port);
    let client = ClientApiClient::connect(addr)
        .await
        .with_context(|| format!("Failed to connect to client API server at port {}", port))?;
    
    Ok(client)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    info!("Starting rpccli");

    // Parse command line arguments
    let args = Args::parse();

    match args.command {
        Commands::Read => {
            info!("Executing read command");
            let mut client = create_client(args.port).await?;
            
            // Execute the read RPC call
            let response = match client.read(ReadMessage {}).await {
                Ok(response) => response,
                Err(status) => {
                    // Check for common errors and provide more user-friendly messages
                    if status.code() == tonic::Code::Cancelled || status.code() == tonic::Code::Internal {
                        println!("The client is in an initialization state or recovering from errors.");
                        println!("Hint: The client node is likely still starting up or is missing data.");
                        println!("Wait a moment and try again, or check the client logs for errors.");
                        return Ok(());
                    } else {
                        return Err(anyhow::anyhow!("Failed to execute read RPC: {}", status));
                    }
                }
            };
            
            // If we get here, we have a valid response
            let pod_data = match response.into_inner().pod_data {
                Some(data) => data,
                None => {
                    println!("Successfully connected, but no pod data is available yet.");
                    println!("Try sending some data with: just rpccli-send \"Your transaction data\"");
                    return Ok(());
                }
            };
            
            // Pretty print the response
            println!("\n=== Pod Network State ===");
            println!("Past Perfect timestamp: {}", pod_data.rperf);
            println!("\nTransactions:");
            
            if pod_data.t.is_empty() {
                println!("  No transactions found");
            } else {
                for (i, record) in pod_data.t.iter().enumerate() {
                    let tx_data = if let Some(tx) = &record.transaction {
                        String::from_utf8_lossy(&tx.data)
                    } else {
                        "[No data]".into()
                    };
                    
                    println!("Transaction #{}", i + 1);
                    println!("  Data: {}", tx_data);
                    println!("  Min timestamp: {}", record.rmin);
                    println!("  Max timestamp: {}", record.rmax);
                    println!("  Confirmed timestamp: {}", 
                        record.rconf.map_or("Not confirmed".to_string(), |ts| ts.to_string()));
                    println!();
                }
            }
        },
        
        Commands::Send { transaction_data } => {
            info!("Executing send command with data: {}", transaction_data);
            let mut client = create_client(args.port).await?;
            
            // Create a transaction from the data string
            let transaction = Transaction {
                data: transaction_data.into_bytes(),
            };
            
            // Execute the write RPC call
            let response = match client.write(WriteMessage {
                transaction: Some(transaction),
            }).await {
                Ok(response) => response,
                Err(status) => {
                    // Check for common errors and provide more user-friendly messages
                    if status.code() == tonic::Code::Cancelled || status.code() == tonic::Code::Internal {
                        println!("Unable to send transaction - client node appears to be in an initialization state.");
                        println!("Hint: The client node is likely still starting up or recovering from errors.");
                        println!("Wait a moment and try again, or check the client logs for errors.");
                        return Ok(());
                    } else {
                        return Err(anyhow::anyhow!("Failed to execute send RPC: {}", status));
                    }
                }
            };
            
            if response.into_inner().success {
                println!("Transaction sent successfully");
            } else {
                println!("Transaction failed to send");
            }
        },
    }

    Ok(())
}
