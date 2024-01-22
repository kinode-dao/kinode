# Eth Provider Process Design

Eth Provider process sits in the middle of the RPC connection for basic reads and eventually writes to the blockchain

If you imagine writing an application using a Provider as they come in Javascript or any other language, you might instantiate one provider at the top of the application.

The purpose of this module is to behave similarly to that. It's the Provider for the entirety of every node's applications.

It receives requests from a variety of processes to perform some RPC call to the blockchain. It coordinates making all of those calls for each of the processes. There could be 50 processes making a variety of requests at any given time and the Provider process will make these requests to its RPC connection and manage returning all of the responses to the processes that requested them.


Here are the three parts and each of their roles:

1. Eth Runtime.
    * Knows where to fulfill the RPC request. 
        * External URL? 
        * Another node on the network? 
        * Locally running ETH node
2. Provider Process.
    * Builds RPC requests that will be sent to the RPC endpoint
    * Interacts with all processes and codes the JSON requests accordingly
    * Receives responses and dispatches them back to their relative packets
3. Process lib imports for arbitrary processes.
    * Idiomatic Request imports like SubscribeLogsRequest.
