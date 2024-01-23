# Eth Provider Process Design

Eth Provider process sits in the middle of the RPC connection for basic reads and eventually writes to the blockchain

If you imagine writing an application using a Provider as they come in Javascript or any other language, you might instantiate one provider at the top of the application.

The purpose of this module is to behave similarly to that. It's the Provider for the entirety of every node's applications.

It receives requests from a variety of processes to perform some RPC call to the blockchain. It coordinates making all of those calls for each of the processes. There could be 50 processes making a variety of requests at any given time and the Provider process will make these requests to its RPC connection and manage returning all of the responses to the processes that requested them.


Below are the components and their roles

1. Provider Process.
    * Upon boot, it waits for a message from the kernel informing it of the RPC endpoints, which may come in the form of Kinode addresses or an http/ws endpoint.
    * Builds RPC requests that will be sent to the RPC endpoint.
    * Interacts with all processes and coordinates those various JSON requests and responses.
2. Process lib imports for arbitrary processes.
    * Idiomatic imports for interacting with the chain
        * SubscribeLogsReuqest. Calls two RPC methods retrieving events for an address:
            * eth_getLogs - get all logs for a given address
            * eth_subscribe - get all future logs
        * CallMethodRequest 
            * eth_call - call a view function on the chain
        * GetStorage 
            * eth_getStorageAt - retrieve value at storage slot
        * SendTransaction 
            * eth_sendTransaction - send a transaction to the mempool

## Handling the RPC status of a node - local or remote?

The Provider must know where to fulfill the requests it is receiving and there's two options on how it can do that: over the uqbar network or directly to an external rpc url it contains.