# Netbench Sync

A simple run of Netbench with even a single client and server requires
the user to:

1. Start the server
2. Start the client
3. Manually Kill the server

In more complex setups with more servers, clients and even routers this
job will get more complex.

There are a couple of pitfalls when attempting to automate this setup:
1. If a client is started before the server it connects to is up the
client will fail.
2. If the client starts long after the server is already up the performance
data collected by a server will start with a long section of useless data.
3. The server doesn't know when the client is finished. In principle the
server could go on to server traffic to many clients. In practice the scenario
file will tell us all the clients that will connect for the lifetime of the
test.

Netbench Sync solves these problems by opening a simple very low
bandwidth line of communication between the client and the server to
communicate status. By communicating status we coordinate starting and
stopping the servers.

Client
 - not-ready (or non-responsive)
 - ready
    - Poll for Server "running" state
 - running
 - finished

Server
 - not-ready (or non-responsive)
 - ready
    - Poll for Client "ready" state
 - running
    - Delayed long poll for Client "finished" state (or non-responsive)
      Delayed to not interfere with test
 - finished