# Podnet
This is an implementation of some of the concepts from the paper [Pod: An Optimal-Latency, Censorship-Free, and Accountable Generalized Consensus Layer](pod-core.pdf)which present an alternative to blockchains for a decentralized transaction ledger.

It's by no means a complete implementation. My initial plan is to get a minimum viable implementation that is good enough to be used as a benchmark target. My curiosity is what the performance would be like. 

## Overview

### Client

The client subproject implements the interaction layer between users and the Pod network. It's built around the ClientAPI protocol buffer service, which provides two main RPC methods:

1. **Write** - Submits transactions to the network
   - Takes a `WriteMessage` containing transaction data
   - Returns a `WriteReply` with success status

2. **Read** - Retrieves current state from the network
   - Takes an empty `ReadMessage` 
   - Returns a `ReadReply` with `PodData` containing:
     - Transaction records (`TxRecord`)
     - Performance metrics (`rperf`)
     - Consensus proof votes (`cpp`)

The design focuses on simplicity and efficiency, with structured data types for transactions, votes, etc. Each transaction record maintains metadata about its confirmation status and associated votes, enabling clients to verify the network's consensus state.

### Replica

The replica node is the core component of the Pod Network, responsible for:

1. **Vote Generation and Consensus**: Replicas vote on transactions, creating a consensus through the log_vote_service that validates and sequences transactions.

2. **Memory-mapped Log Storage**: Uses memory-mapped files for efficient, concurrent log storage with atomic appends through the replica_log implementation.

3. **WebSocket Communication**: Maintains connections with clients via client_comms_service, receiving transactions and broadcasting votes.

4. **Heartbeat Mechanism**: Implements a heartbeat_service that ensures liveness and helps detect network partitions.

5. **Ed25519 Cryptography**: Uses Ed25519 signatures for transaction authentication and vote validation.

6. **Asynchronous Architecture**: Built on Tokio with broadcast/mpsc channels for efficient inter-service communication.

7. **Round-based Processing**: Organizes transaction processing into time-based rounds (ROUND_SIZE_MS).

The design prioritizes performance, fault tolerance, and simplicity while maintaining a simple implementation that can be used as a benchmarking target.







## Instructions
Install the taks runner:
```bash
cargo install just
```

Read through the available commands in the task definition file `justfile` to learn the available commands.

## Example
```bash
just minimal-network
```
Starts a minimal network with 1 replica and 1 client. 

```bash
just network-multi
```
Starts a network with 5 replicas and 3 clients.
You can use the client rpccli to send transactions to the network.

```bash
just rpccli-send "transaction data string"
just rpccli-read
```

To stop the network run:
```bash
just stop-network
```