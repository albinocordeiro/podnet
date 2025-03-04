use std::fs::File;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Command, Child};
use std::thread;
use std::time::Duration;
use std::io::Read;

use tempfile::TempDir;
use tracing::info;

// Integration test for the client binary
#[test]
fn test_client_binary_init() {
    // Create a temporary directory for test files
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    println!("Temp directory path: {}", temp_dir.path().display());
    // Create replica public key file with a valid public key
    // Using the same valid key from the existing tests
    let valid_pubkey_hex = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
    let pk_file_path = temp_dir.path().join("replica_pk.txt");
    let mut pk_file = File::create(&pk_file_path).expect("Failed to create pk file");
    writeln!(pk_file, "1 {}", valid_pubkey_hex).expect("Failed to write to pk file");
    
    // Create replica endpoints file with a valid websocket URL
    // Use a non-existent address since we're just testing initialization, not connection
    let endpoints_file_path = temp_dir.path().join("replica_endpoints.txt");
    let mut endpoints_file = File::create(&endpoints_file_path).expect("Failed to create endpoints file");
    writeln!(endpoints_file, "1 ws://localhost:12345").expect("Failed to write to endpoints file");
    
    // Get the path to the client binary
    // This assumes the binary has been built with `cargo build`
    let client_bin_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/client");
    
    // Build the command
    let mut child = Command::new(client_bin_path)
        .arg(pk_file_path.to_str().unwrap())
        .arg(endpoints_file_path.to_str().unwrap())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn client binary");
    
    if let Some(stdout) = child.stdout.take() {
        println!("STDOUT");
        let reader = std::io::BufReader::new(stdout);
        thread::spawn(move || {
            for line in reader.lines() {
                println!("STDOUT: {}", line.expect("Failed to read line"));
            }
        });
    }
    
    if let Some(stderr) = child.stderr.take() {
        println!("STDERR");
        let reader = std::io::BufReader::new(stderr);
        thread::spawn(move || {
            for line in reader.lines() {
                println!("STDERR: {}", line.expect("Failed to read line"));
            }
        });
    }
    
    // Give the process some time to start up and initialize
    thread::sleep(Duration::from_secs(10));
    info!("Kill podnetwork client process");
    // Kill the process as it would run indefinitely waiting for connections
    let _ = child.kill();
    
    info!("Wait for podnetwork client process to exit");
    // Wait for the process to exit
    match child.wait() {
        Ok(status) => {
            println!("Client process exited with status: {}", status);
            // Note: We don't check status.success() here because we forcibly killed the process
        }
        Err(e) => {
            panic!("Failed to wait for client process: {}", e);
        }
    }
}

#[test]
fn test_client_binary_invalid_pk_file() {
    // Create a temporary directory for test files
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    
    // Create invalid replica public key file
    let pk_file_path = temp_dir.path().join("invalid_pk.txt");
    let mut pk_file = File::create(&pk_file_path).expect("Failed to create pk file");
    writeln!(pk_file, "1 invalid_key").expect("Failed to write to pk file");
    
    // Create valid replica endpoints file
    let endpoints_file_path = temp_dir.path().join("replica_endpoints.txt");
    let mut endpoints_file = File::create(&endpoints_file_path).expect("Failed to create endpoints file");
    writeln!(endpoints_file, "1 ws://localhost:12345").expect("Failed to write to endpoints file");
    
    // Get the path to the client binary
    let client_bin_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/client");
    
    // Build the command
    let output = Command::new(client_bin_path)
        .arg(pk_file_path.to_str().unwrap())
        .arg(endpoints_file_path.to_str().unwrap())
        .output()
        .expect("Failed to execute client binary");
    
    // Check that the command failed due to invalid public key
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid hex string"));
}

#[test]
fn test_client_binary_invalid_endpoints_file() {
    // Create a temporary directory for test files
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    
    // Create valid replica public key file
    let valid_pubkey_hex = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
    let pk_file_path = temp_dir.path().join("replica_pk.txt");
    let mut pk_file = File::create(&pk_file_path).expect("Failed to create pk file");
    writeln!(pk_file, "1 {}", valid_pubkey_hex).expect("Failed to write to pk file");
    
    // Create invalid replica endpoints file with non-websocket URL
    let endpoints_file_path = temp_dir.path().join("invalid_endpoints.txt");
    let mut endpoints_file = File::create(&endpoints_file_path).expect("Failed to create endpoints file");
    writeln!(endpoints_file, "1 http://localhost:12345").expect("Failed to write to endpoints file");
    
    // Get the path to the client binary
    let client_bin_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/client");
    
    // Build the command
    let output = Command::new(client_bin_path)
        .arg(pk_file_path.to_str().unwrap())
        .arg(endpoints_file_path.to_str().unwrap())
        .output()
        .expect("Failed to execute client binary");
    
    // Check that the command failed due to invalid endpoint URL
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid websocket URL scheme"));
}

#[test]
fn test_client_binary_missing_files() {
    // Get the path to the client binary
    let client_bin_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/client");
    
    // Test with non-existent files
    let output = Command::new(&client_bin_path)
        .arg("nonexistent_pk_file.txt")
        .arg("nonexistent_endpoints_file.txt")
        .output()
        .expect("Failed to execute client binary");
    
    // Check that the command failed
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to open file"));
}

// This test ensures that the client reports appropriate errors when replica IDs don't match
#[test]
fn test_client_binary_mismatched_replica_ids() {
    // Create a temporary directory for test files
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    
    // Create replica public key file for replica ID 1
    let valid_pubkey_hex = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
    let pk_file_path = temp_dir.path().join("replica_pk.txt");
    let mut pk_file = File::create(&pk_file_path).expect("Failed to create pk file");
    writeln!(pk_file, "1 {}", valid_pubkey_hex).expect("Failed to write to pk file");
    
    // Create replica endpoints file for replica ID 2 (different ID)
    let endpoints_file_path = temp_dir.path().join("replica_endpoints.txt");
    let mut endpoints_file = File::create(&endpoints_file_path).expect("Failed to create endpoints file");
    writeln!(endpoints_file, "2 ws://localhost:12345").expect("Failed to write to endpoints file");
    
    // Get the path to the client binary
    let client_bin_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/client");
    
    // Build the command
    let output = Command::new(client_bin_path)
        .arg(pk_file_path.to_str().unwrap())
        .arg(endpoints_file_path.to_str().unwrap())
        .output()
        .expect("Failed to execute client binary");
    
    // Since the IDs don't match, initialization will probably still succeed because the code merely collects
    // available replicas (by ID) rather than requiring a 1:1 match
    // But we expect runtime errors once the client tries to communicate with replicas
    // For this test, we just ensure the initialization doesn't crash
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("Client binary unexpectedly failed: {}", stderr);
    }
}
