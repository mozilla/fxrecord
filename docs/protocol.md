# fxrecord protocol

The fxrecord protocol is broken up into a number of sections:

1. Handshake
2. DownloadBuild
3. SendProfile
4. SendPrefs

## Message Format

Messages are encoded as JSON blobs (via Serde). Each message is prefixed with
a 4-byte length.

Example:

```
00 00 00 1F # Length of Message (31)
{"Handshake":{"restart":false}}
```

In replies, it is common for the recorder to send a `Result` back. If the
result is `Ok`, then this indicates that the corresponding request was
successful. However, if an `Err` is returned to the recorder, then a fatal
error has occurred and the protocol cannot continue. At this point, the
recorder and runner will disconnect from eachother.

An example of protocol failure can be seen below in Figure 2.

## 1. Handshake

The protocol is initiated by the recorder connecting to the runner over TCP.
The recorder will send a `Handshake` message to the runner, indicating that
it should restart. The runner replies with a `HandshakeReply` with the status
of the restart operation. They then disconnect and the recorder waits for the
runner to restart.

> ![](/docs/diagrams/handshake.png)
>
> Figure 1: Handshake

If something goes wrong with the handshake on the runner's end (such as a
failure with the Windows API when attempting to restart), it will instead
reply with an error message inside its `HandshakeReply`:

> ![](/docs/diagrams/handshake-failure.png)
>
> Figure 2: Handshake Failure

If the recorder requested a restart, it will then attempt to reconnect to the
runner with exponential backoff and handshake again, this time not requesting
a restart

## 2. DownloadBuild

After reconnecting, the next message from the recorder will be for the runner
to download a specific build of Firefox from Taskcluster.

> ![](/docs/diagrams/download-build.png)
>
> Figure 3: Download Build
