format:
    cargo fmt --all -- --check
fix:
    cargo fix --all

# Build the client binary
build-client:
    cargo build --package client --bin client

# Run client with default configuration
run-client-defaults:
    RUST_LOG=debug cargo run --package client --bin client -- "replicas_config.json"

# Run client with custom configuration
run-client-custom ALPHA BETA REPLICAS_CONFIG_FILE:
    RUST_LOG=debug cargo run --package client --bin client -- {{REPLICAS_CONFIG_FILE}} --alpha {{ALPHA}} --beta {{BETA}}

# Run client tests
client-tests:
    RUST_LOG=debug cargo test --package client

# Generate replica configuration with default settings (3 replicas)
gen-replica-config:
    RUST_LOG=debug cargo run --package common --bin generate_replica_config

# Generate replica configuration with custom number of replicas
gen-replica-config-count COUNT:
    RUST_LOG=debug cargo run --package common --bin generate_replica_config -- --count {{COUNT}}

# Generate replica configuration with custom settings
gen-replica-config-custom COUNT BASE_URL OUTPUT PRIVKEY_OUTPUT:
    RUST_LOG=debug cargo run --package common --bin generate_replica_config -- --count {{COUNT}} --base-url {{BASE_URL}} --output {{OUTPUT}} --privkey-output {{PRIVKEY_OUTPUT}}

# Build the replica binary
build-pod-replica:
    cargo build --package pod --bin replica

# Run replica with default configuration
run-pod-node-defaults:
    RUST_LOG=debug cargo run --package pod --bin replica

# Build the rpccli binary
build-rpccli:
    cargo build --package client --bin rpccli

# Run rpccli read command (display Pod Network state)
rpccli-read:
    RUST_LOG=debug cargo run --package client --bin rpccli -- read

# Run rpccli send command (submit a transaction)
rpccli-send TX_DATA:
    RUST_LOG=debug cargo run --package client --bin rpccli -- send "{{TX_DATA}}"

# Run rpccli with a specific port (for multi-client setups)
rpccli-read-port PORT:
    RUST_LOG=debug cargo run --package client --bin rpccli -- --port {{PORT}} read

# Run rpccli send command on a specific port
rpccli-send-port PORT TX_DATA:
    RUST_LOG=debug cargo run --package client --bin rpccli -- --port {{PORT}} send "{{TX_DATA}}"

# Start a minimal Pod Network with one replica and one client
# This is useful for testing and development
minimal-network:
    @echo "=== Starting a minimal Pod Network with 1 replica and 1 client ==="
    @echo "Cleaning up any previous instances..."
    just stop-network
    rm -f replica_0.log client_0.log
    
    @echo "Generating replica configuration..."
    just gen-replica-config-count 1
    
    @echo "Cleaning up any existing replica log files..."
    rm -f replica_wal_0
    
    @echo "Starting replica in the background..."
    RUST_LOG=debug cargo run --package pod --bin replica -- --replica-id=0 --port=12345 --replica-log-file=replica_wal_0 --private-key="$(cat private_keys.json | jq -r '.[0].privk')" > replica_0.log 2>&1 &
    
    @echo "Sleeping 5 seconds to ensure replica is ready..."
    sleep 5
    
    @echo "Checking if replica is running properly..."
    if ! grep -q "WebSocket server listening" replica_0.log; then \
        echo "ERROR: Replica failed to start. Check replica_0.log for details"; \
        just stop-network; \
        exit 1; \
    fi
    
    @echo "Starting client in the background..."
    RUST_LOG=debug cargo run --package client --bin client -- replicas_config.json --port=50051 > client_0.log 2>&1 &
    
    @echo "Sleeping 5 seconds to ensure client is ready..."
    sleep 5
    
    @echo "Checking if client is running properly..."
    if ! grep -q "Starting RPC API server" client_0.log; then \
        echo "ERROR: Client failed to start. Check client_0.log for details"; \
        just stop-network; \
        exit 1; \
    fi
    
    @echo "=== Minimal network is now running ==="
    @echo "View logs with: tail -f replica_0.log client_0.log"
    @echo "Interact with the network using the rpccli tool:"
    @echo "  just rpccli-read                 # View current network state"
    @echo "  just rpccli-send \"your message\"  # Send a transaction"
    @echo "Press Ctrl+C to stop the network"
    @echo "To stop the network run: just stop-network"

# Start a network with 5 replicas and 3 clients
# This is useful for more advanced testing scenarios
network-multi:
    @echo "=== Starting a multi-node Pod Network with 5 replicas and 3 clients ==="
    @echo "Stopping any previous instances..."
    just stop-network
    rm -f replica_*.log client_*.log
    
    @echo "Generating replica configuration..."
    just gen-replica-config-count 5
    
    @echo "Cleaning up any existing replica log files..."
    rm -f replica_wal_*
    
    @echo "Starting replicas in the background..."
    # Start replica 0
    RUST_LOG=debug cargo run --package pod --bin replica -- --replica-id=0 --port=12345 --replica-log-file=replica_wal_0 --private-key="$(cat private_keys.json | jq -r '.[0].privk')" > replica_0.log 2>&1 &
    # Start replica 1
    RUST_LOG=debug cargo run --package pod --bin replica -- --replica-id=1 --port=12346 --replica-log-file=replica_wal_1 --private-key="$(cat private_keys.json | jq -r '.[1].privk')" > replica_1.log 2>&1 &
    # Start replica 2
    RUST_LOG=debug cargo run --package pod --bin replica -- --replica-id=2 --port=12347 --replica-log-file=replica_wal_2 --private-key="$(cat private_keys.json | jq -r '.[2].privk')" > replica_2.log 2>&1 &
    # Start replica 3
    RUST_LOG=debug cargo run --package pod --bin replica -- --replica-id=3 --port=12348 --replica-log-file=replica_wal_3 --private-key="$(cat private_keys.json | jq -r '.[3].privk')" > replica_3.log 2>&1 &
    # Start replica 4
    RUST_LOG=debug cargo run --package pod --bin replica -- --replica-id=4 --port=12349 --replica-log-file=replica_wal_4 --private-key="$(cat private_keys.json | jq -r '.[4].privk')" > replica_4.log 2>&1 &
    
    @echo "Sleeping 5 seconds to ensure replicas are ready..."
    sleep 5
    
    @echo "Checking if replicas are running properly..."
    # Check replica 0
    if ! grep -q "WebSocket server listening" replica_0.log; then \
        echo "ERROR: Replica 0 failed to start. Check replica_0.log for details"; \
        just stop-network; \
        exit 1; \
    fi
    echo "Replica 0 is running."
    # Check replica 1
    if ! grep -q "WebSocket server listening" replica_1.log; then \
        echo "ERROR: Replica 1 failed to start. Check replica_1.log for details"; \
        just stop-network; \
        exit 1; \
    fi
    echo "Replica 1 is running."
    # Check replica 2
    if ! grep -q "WebSocket server listening" replica_2.log; then \
        echo "ERROR: Replica 2 failed to start. Check replica_2.log for details"; \
        just stop-network; \
        exit 1; \
    fi
    echo "Replica 2 is running."
    # Check replica 3
    if ! grep -q "WebSocket server listening" replica_3.log; then \
        echo "ERROR: Replica 3 failed to start. Check replica_3.log for details"; \
        just stop-network; \
        exit 1; \
    fi
    echo "Replica 3 is running."
    # Check replica 4
    if ! grep -q "WebSocket server listening" replica_4.log; then \
        echo "ERROR: Replica 4 failed to start. Check replica_4.log for details"; \
        just stop-network; \
        exit 1; \
    fi
    echo "Replica 4 is running."
    
    @echo "Starting clients in the background..."
    # Start client 0
    RUST_LOG=debug cargo run --package client --bin client -- replicas_config.json --port=50051 > client_0.log 2>&1 &
    # Start client 1
    RUST_LOG=debug cargo run --package client --bin client -- replicas_config.json --port=50052 > client_1.log 2>&1 &
    # Start client 2
    RUST_LOG=debug cargo run --package client --bin client -- replicas_config.json --port=50053 > client_2.log 2>&1 &
    
    @echo "Sleeping 5 seconds to ensure clients are ready..."
    sleep 5
    
    @echo "Checking if clients are running properly..."
    # Check client 0
    if ! grep -q "Starting RPC API server" client_0.log; then \
        echo "ERROR: Client 0 failed to start. Check client_0.log for details"; \
        just stop-network; \
        exit 1; \
    fi
    echo "Client 0 is running."
    # Check client 1
    if ! grep -q "Starting RPC API server" client_1.log; then \
        echo "ERROR: Client 1 failed to start. Check client_1.log for details"; \
        just stop-network; \
        exit 1; \
    fi
    echo "Client 1 is running."
    # Check client 2
    if ! grep -q "Starting RPC API server" client_2.log; then \
        echo "ERROR: Client 2 failed to start. Check client_2.log for details"; \
        just stop-network; \
        exit 1; \
    fi
    echo "Client 2 is running."
    
    @echo "=== Multi-node network is now running ==="
    @echo "View logs with: tail -f replica_*.log client_*.log"
    @echo "Interact with the network using the rpccli tool:"
    @echo "  just rpccli-read                 # View current network state (uses client 0)"
    @echo "  just rpccli-send \"your message\"  # Send a transaction"
    @echo "To interact with a specific client, use:"
    @echo "  just rpccli-read-port 50052      # View state from client 1"
    @echo "  just rpccli-send-port 50053 \"message\"  # Send via client 2"
    @echo "To stop the network run: just stop-network"

# Stop all running network components
stop-network:
    @echo "Stopping all Pod Network components..."
    pkill -f "target/debug/replica" || true
    pkill -f "target/debug/client" || true
    @echo "All components stopped"