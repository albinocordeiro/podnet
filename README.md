# Instructions
Install the taks runner:
```bash
cargo install just
```

Read through the available commands in the task definition file `justfile` to learn the available commands.

# Example
```bash
just minimal-network
```
Starts a minimal network with 1 replica and 1 client. 
You can use the client rpccli to send transactions to the network.

```bash
just rpccli send "your message"
```



```bash
just stop-network
