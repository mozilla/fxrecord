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

The only supported operating system for `fxrunner` is Windows 10. It is a
statically linked executable with no other dependencies.

`fxrecorder` is also a statically linked executable, but it has two
additional requirements:

* a capture device compatible with [ffmpeg][ffmpeg] (I am using an
  [AverMedia Gc551][gc551]); and
* [ffmpeg][ffmpeg] version 4.2 or newer.

`fxrecorder` presently requires a Windows 10 device as it assumes that
DirectShow will be used for video capture. The capture card used while
developing `fxrecord` only supported Windows 10 and therefore was not able to
be tested against any other operating system.

[ffmpeg]: https://ffmpeg.org
[gc551]: https://www.avermedia.com/us/product-detail/GC551


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

[fxrecorder.recording]
# The path to ffmpeg on the file system.
ffmpeg_path = "C:\ffmpeg\ffmepg.exe"

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