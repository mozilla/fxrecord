# fxrecord

`fxrecord` is a tool for capturing and analyzing recordings of Firefox
desktop startup. It consists of two parts: `fxrecorder`, which records the
output of the reference hardware and does analysis, and `fxrunner`, which
instruments Firefox on the reference hardware.

## License

`fxrecord` is released under the terms of the [Mozilla Public License 2.0](LICENSE).

## Code of Conduct

This repository follows a [code of conduct](CODE_OF_CONDUCT.md).

## Requirements

Both `fxrunner` and `fxrecorder` require Rust 1.39+ for async/await support.

### fxrunner

The only supported operating system for `fxrunner` is Windows 10. It is a
statically linked executable with no other dependencies. Additionally, it
requires legacy IO counters to be enabled. This can be done by running the
following once:

```
diskperf -Y
```

### fxrecorder

`fxrecorder` is also a statically linked executable, but it has two
additional requirements:

- a capture device compatible with [ffmpeg][ffmpeg] (I am using an
  [AverMedia Gc551][gc551]);
- [ffmpeg][ffmpeg] version 4.2 or newer; and
- [ImageMagick][imagemagick] version 6.9.

`fxrecorder` presently requires a Windows 10 device as it assumes that
DirectShow will be used for video capture. The capture card used while
developing `fxrecord` only supported Windows 10 and therefore was not able to
be tested against any other operating system.

[ffmpeg]: https://ffmpeg.org
[gc551]: https://www.avermedia.com/us/product-detail/GC551
[imagemagick]: https://legacy.imagemagick.org/

## Building

`fxrecord` is built in Rust and uses [Cargo][rustup]. To build for production
use, run

```sh
cargo build --release
```

and the binaries will be located in the `target/release/` subdirectory. Copy
the `fxrunner` binary to the reference device and the `fxrecorder` to the
recording device.

[rustup]: https://rustup.rs/

## Configuration

`fxrunner` requires a configuration file named fxrecord.toml with
`[fxrunner]` and `[fxrecorder.recording]` sections:

```toml
[fxrecorder]
# The host and port that FxRunner is listening on.
host = "127.0.0.1:8888"
visual_metrics_path = "vendor\\visualmetrics.py"

[fxrecorder.recording]
# The size of the video stream.
video_size = { y = 1920, x = 1080 }

# The desired size. If omitted, video_size will be used instead.
output_size = { y = 1366, x = 768 }

# The frame rate to capture at.
frame_rate = 60

# The name of the device to use for capture.
device = "AVerMedia GC551 Video Capture"

# The size of the buffer to use while streaming frames.
buffer_size = "1000M"
```

`fxrunner` requires a configuration file named `fxrecord.toml` with a
`[fxrunner]` section:

```toml
[fxrunner]
# The host and port that FxRecorder will be able to connect to.
host = "0.0.0.0:8888"
```

An [example configuration](fxrecord.example.toml) is provided.

## Testing

To run the unit tests, run `cargo test`.

The integration tests support logging, but by default cargo captures test
output and runs tests in parallel. To have logs associated with tests
clearly, run:

```sh
cargo test -p integration-tests -- --nocapture --test-threads 1
```

to force unit tests to run sequentially.

## Deployment

Deployment is done with the `Deploy.ps1` script.

Example:

```ps1
# Deploy fxrecorder the host-specific configuration.
.\contrib\Deploy.ps1 `
    -HostName fxrecorder01.corp.tor1.mozilla.com `
    -UserName fxrecord `
    -MachineType recorder

# Deploy fxrunner and the host-specific configuration.
.\contrib\Deploy.ps1 `
    -HostName fxrunner01.corp.tor1.mozilla.com `
    -UserName fxrunner `
    -MachineType runner
```
