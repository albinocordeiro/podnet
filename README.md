# Podnet
This is an implementation of some of the concepts from the paper [Pod: An Optimal-Latency, Censorship-Free, and Accountable Generalized Consensus Layer](pod-core.pdf)which present an alternative to blockchains for a decentralized transaction ledger.

## Overview





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