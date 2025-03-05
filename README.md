# PodNet: A Distributed Replicated State Machine System

PodNet is a Rust-based distributed system that implements a replicated state machine using a network of pod nodes (replicas). The system provides fault tolerance and consistency guarantees through a carefully designed consensus protocol.

## Table of Contents

- [Overview](#overview)
- [Prerequisites](#prerequisites)
- [Project Structure](#project-structure)
- [Getting Started](#getting-started)
  - [Building the Project](#building-the-project)
  - [Running a Minimal Network](#running-a-minimal-network)
  - [Running a Multi-Node Network](#running-a-multi-node-network)
- [Configuration](#configuration)
- [Interacting with PodNet](#interacting-with-podnet)
- [Testing](#testing)
- [Advanced Usage](#advanced-usage)
- [Architecture](#architecture)
- [Development](#development)

## Overview
PodNet is designed as a distributed network of replicas (pods) that maintain consistent state across the network. Clients connect to this network to submit transactions and read the current state. The system uses a consensus algorithm with configurable parameters (alpha and beta) to ensure consistency and fault tolerance.

## Prerequisites
- Rust and Cargo (latest stable version)
- Just command runner (`cargo install just`)
- jq for JSON processing in scripts (`brew install jq` on macOS)

## Project Structure
PodNet is organized as a Cargo workspace with three main crates:
- `client`: Client implementation for interacting with the pod network
- `pod`: The replica/node implementation that forms the core of the network
- `common`: Shared components and utilities used by both client and pod

## Getting Started
### Building the Project
Build all components:
```bash
cargo build
```
Or build specific components:
```bash
# Build just the client
just build-client

# Build just the replica
just build-pod-replica

# Build the CLI tool
just build-rpccli
```

### Running a Minimal Network
The easiest way to get started is to run a minimal network with one replica and one client:
```bash
just minimal-network
```
This command:
1. Generates the necessary configurations
2. Starts a single replica
3. Starts a client connected to that replica
4. Validates that everything is running correctly

The network will run in the background. You can monitor the logs with:
```bash
tail -f replica_0.log client_0.log
```
To stop the network:
```bash
just stop-network
```

### Running a Multi-Node Network
For a more realistic setup with multiple replicas and clients:
```bash
just network-multi
```
This starts 5 replicas and 3 clients, demonstrating the distributed nature of PodNet.

## Configuration
### Generating Replica Configuration
Before running the system, you need a configuration file for the replicas. This can be generated with:
```bash
# Default configuration (3 replicas)
just gen-replica-config

# Custom number of replicas
just gen-replica-config-count 5

# Advanced configuration
just gen-replica-config-custom 5 "ws://localhost:123" "custom_config.json" "custom_keys.json"
```
This creates:
- `replicas_config.json`: Contains the network configuration for replicas
- `private_keys.json`: Contains the private keys for each replica

### Client Configuration
Clients can be configured with different parameters:
```bash
# Run client with default configuration
just run-client-defaults

# Run client with custom alpha and beta parameters
just run-client-custom 5 1 replicas_config.json
```
The alpha and beta parameters control the consensus algorithm:
- `alpha`: Number of timestamps to consider (must be ≥ 4β + 1)
- `beta`: Number of padding values

## Interacting with PodNet
Once your network is running, you can interact with it using the RPC CLI tool:
```bash
# View the current state of the network
just rpccli-read

# Submit a transaction to the network
just rpccli-send "Your transaction data here"

# Interact with a specific client (when running multiple clients)
just rpccli-read-port 50052
just rpccli-send-port 50052 "Your transaction data here"
```

## Testing
Run the client tests:
```bash
just client-tests
```
For more comprehensive testing, you can:
1. Start a network (minimal or multi-node)
2. Submit transactions using the rpccli tool
3. Read the state to verify correct behavior
4. Check the logs for detailed information

## Advanced Usage
### Custom Replica Setup
You can start individual replicas with specific configurations:
```bash
RUST_LOG=debug cargo run --package pod --bin replica -- \
  --replica-id=0 \
  --port=12345 \
  --replica-log-file=replica_wal_0 \
  --private-key="$(cat private_keys.json | jq -r '.[0].privk')"
```

### Custom Client Setup
Similarly, you can start a client with specific parameters:
```bash
RUST_LOG=debug cargo run --package client --bin client -- \
  replicas_config.json --alpha 5 --beta 1 --port 50051
```

## Architecture
PodNet consists of:
1. **Replicas (Pods)**: Form the core of the network and maintain consistent state through consensus
2. **Clients**: Connect to the replica network, submit transactions, and read state
3. **RPC CLI**: Command-line tool for interacting with the network

Communication:
- Replicas communicate with each other via WebSockets
- Clients connect to replicas through a custom protocol
- The RPC CLI tool connects to clients via gRPC

## Development
For development and debugging:
1. Set the log level with the `RUST_LOG` environment variable (e.g., `RUST_LOG=debug`)
2. Use the minimal network setup for rapid iteration
3. Monitor logs for detailed information about the system's behavior
4. Run individual components for focused development

The codebase follows standard Rust practices with proper error handling, modularity, and testing.