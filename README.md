# rustmpeg

A clean-room, from-scratch multimedia framework in Rust targeting command-line and functional parity with FFmpeg while keeping the Rust source tree free of third-party crate dependencies.

> Status: early implementation. This repository does **not** claim FFmpeg feature parity yet. Parity is earned subsystem-by-subsystem and tracked explicitly.

## Baseline

- Compatibility target: FFmpeg 9.0.1 "Lei"
- Rust baseline: Rust 1.98.0 stable
- Edition: Rust 2024
- Dependency policy: `std` + repository-owned code only
- Unsafe policy: denied by default; allowed only in narrowly-audited platform/SIMD/FFI modules
- Primary binaries: `ffmpeg`, `ffprobe`, `ffplay`

## What "drop-in" means

The project has four independent parity gates:

1. **CLI parity** — accepted option grammar, stream specifiers, mappings, exit codes, diagnostics and output conventions.
2. **Media parity** — containers, codecs, filters, metadata, subtitles, devices, protocols and timing behavior.
3. **API parity layer** — Rust-native APIs first, with an optional C ABI compatibility surface implemented by repository-owned code.
4. **Behavioral parity** — golden tests compare probing, remuxing, transcoding, timestamps, bitstreams and error behavior against the pinned FFmpeg reference.

A component is never marked complete because its name exists in a registry. It must pass its parity fixture set.

## Strict zero-dependency policy

`Cargo.toml` files may not contain third-party entries under `[dependencies]`, `[dev-dependencies]` or `[build-dependencies]`.

Allowed:

- Rust `core`, `alloc` and `std`.
- Code implemented inside this repository.
- Direct operating-system interfaces required for files, networking, devices, graphics, audio, threads and hardware acceleration.
- Architecture intrinsics from `core::arch` / `std::arch`.

Not allowed:

- Codec wrapper crates.
- FFmpeg/libav* linking.
- OpenSSL, zlib, x264, x265, dav1d, libvpx, libaom, SDL or similar libraries as implementation shortcuts.
- Vendoring a third-party library and calling it "source code".

Where FFmpeg normally delegates to an optional external library, rustmpeg must either implement the capability itself or leave the parity item explicitly incomplete until it does.

## Architecture

```text
apps/
  ffmpeg/          compatibility CLI + transcoding orchestration
  ffprobe/         probing/inspection CLI
  ffplay/          player CLI
crates/
  rm-core/         errors, rational math, timestamps, packets, frames, buffers
  rm-io/           byte/bit IO, buffered readers/writers, seek abstractions
  rm-format/       probing, demuxers, muxers, metadata, chapters
  rm-codec/        codec registry and codec implementations
  rm-filter/       audio/video filter graph
  rm-scale/        pixel conversion/scaling
  rm-resample/     audio sample conversion/resampling
  rm-protocol/     file/network protocols
  rm-device/       capture/output devices
  rm-hwaccel/      hardware acceleration interfaces
  rm-cli/          FFmpeg-compatible argument grammar and shared presentation
  rm-abi/          optional C ABI compatibility layer
```

All crates are internal workspace members. Crate boundaries are used to enforce layering, not to pull code from crates.io.

## Data path

```text
Protocol -> buffered IO -> probe -> demux -> packet
                                      |
                                      v
                                    decode
                                      |
                                    frame
                                      |
                                 filter graph
                                      |
                                    encode
                                      |
                                    packet
                                      |
                                     mux
                                      |
                                  protocol
```

### Core design rules

- Borrowed slices are preferred for parsing; allocations happen only when ownership or lifetime requires them.
- Packet/frame payloads use owned reference-counted storage from `std::sync::Arc` where sharing is needed.
- Integer overflow, malformed lengths and timestamp rescaling are checked.
- Parsers are bounded by explicit limits and never trust container sizes blindly.
- Hot loops get scalar reference implementations first, then architecture-specific SIMD implementations with differential tests.
- Threads use `std::thread`; scheduling and work-stealing primitives are repository-owned.
- Network stacks use `std::net` initially. Protocol-specific state machines are implemented in-tree.
- No global mutable codec/format registries. Built-ins are immutable tables or explicit registries.

## Delivery roadmap

### Phase 0 — bootstrap and contracts

- [x] Repository and parity contract
- [x] Rust workspace with zero-dependency enforcement
- [x] Shared error/rational/time model
- [x] byte and bit readers/writers
- [x] capability registry
- [x] CI parity/format/lint gates

### Phase 1 — first complete vertical slice

- [x] RIFF/WAVE probe + demux + mux
- [x] PCM integer/float codecs
- [x] `ffprobe` stream/container reporting
- [x] `ffmpeg -i in.wav -c copy out.wav`
- [x] PCM format conversion
- [ ] extended timestamp and seek parity tests

### Phase 2 — image/video fundamentals

- [x] image2 sequence protocol/demux/mux
- [x] PPM/PGM/PBM P1-P6
- [ ] BMP
- [ ] TGA
- [ ] PNG (including repository-owned DEFLATE/zlib)
- [ ] JPEG baseline/progressive
- [x] packed pixel format model and initial scaler (`gray8`/`rgb24`, nearest-neighbor)

### Phase 3 — foundational compressed audio

- [ ] ADPCM families
- [ ] FLAC
- [ ] MP2/MP3
- [ ] AAC-LC/HE-AAC
- [ ] Vorbis
- [ ] Opus
- [ ] Ogg mux/demux

### Phase 4 — mainstream containers

- [ ] Matroska/WebM
- [ ] ISO BMFF / MOV / MP4
- [ ] MPEG-TS
- [ ] MPEG-PS
- [ ] AVI
- [ ] FLV
- [ ] HLS/DASH manifest handling

### Phase 5 — mainstream video

- [ ] MPEG-1/2 video
- [ ] MPEG-4 Part 2
- [ ] H.263
- [ ] H.264/AVC
- [ ] H.265/HEVC
- [ ] VP8/VP9
- [ ] AV1
- [ ] lossless/pro codecs tracked by the parity matrix

### Phase 6 — filters, subtitles and devices

- [ ] FFmpeg-compatible filter graph parser
- [ ] core audio/video filters
- [ ] subtitle codecs/rendering primitives
- [ ] Linux devices (ALSA/V4L2/DRM as applicable)
- [ ] Windows devices/APIs
- [ ] macOS devices/APIs
- [ ] `ffplay` display/audio synchronization

### Phase 7 — protocols and security

- [ ] HTTP/1.1
- [ ] HTTP/2 where required for parity
- [ ] TLS implemented in-tree for HTTPS parity
- [ ] RTP/RTCP
- [ ] RTSP
- [ ] SRT-equivalent parity item
- [ ] UDP/TCP/Unix/file/pipe/concat/cache/etc.
- [ ] fuzz corpus + adversarial parser suite

### Phase 8 — ABI and long-tail parity

- [ ] C ABI compatibility targets for libavutil/libavcodec/libavformat/libavfilter/libswscale/libswresample/libavdevice
- [ ] remaining formats/codecs/filters/protocols
- [ ] hardware acceleration parity
- [ ] exhaustive option/diagnostic parity

## Definition of done for a feature

A feature may be marked complete only when:

1. The implementation is in-tree and adds no third-party crate dependency.
2. Unit tests cover normal, boundary and malformed inputs.
3. Differential/golden tests exist where an FFmpeg reference behavior is available.
4. The CLI registry advertises it only after the implementation is usable.
5. Its parity row records known deviations.
6. `cargo test --workspace --all-targets` passes.

## Reality of the scope

FFmpeg represents decades of multimedia engineering and a very large codec/container/protocol surface. A true zero-dependency reimplementation with complete parity is therefore a long-running systems project, not a wrapper-sized rewrite. The repository is structured so every commit can increase real parity without pretending unfinished subsystems exist.

## License

License selection is intentionally deferred until the clean-room implementation and desired downstream compatibility rules are fixed. Do not copy FFmpeg source code into this repository.