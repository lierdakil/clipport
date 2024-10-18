# Clipport

Clipboard teleporter. A silly solution to an annoying issue. I occasionally work with remote machines or VMs, and clipboard sync doesn't always work reliably. This is a silly attempt to do clipboard sync as a TCP client/server.

## Usage

On one machine, run `clipport server -p <port>`. On any number of other
machines, run `clipport client <server ip>:<port>`. Your clipboard is now
synced.

For example, on `192.168.0.1` run `clipport server -p 5563`, and on
`192.168.0.2` run `clipport client 192.168.0.1:1234`.

## Caveats

- Barely tested, use at your own risk.
- No authentication, encryption, etc. Only use in trusted environments.
