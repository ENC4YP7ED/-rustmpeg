# PNG depth and Adam7 pre-release matrix

This document is the release contract for the PNG depth/interlace slice. A capability is not marked complete because synthetic unit tests pass; it must satisfy upstream corpus, malformed-input, CLI, and cross-platform checks.

## Upstream acceptance corpus

Primary corpus: PngSuite as mirrored by `pnggroup/libpng`, `golang/go`, and the full basic/interlaced mirror at `lunapaint/pngsuite`.

| Vector | Capability | Required result | Status |
| --- | --- | --- | --- |
| `basn0g01.png` | 1-bit grayscale | decode to `Gray8` | PASS |
| `basn0g02.png` | 2-bit grayscale | decode to `Gray8` | PASS |
| `basn0g04.png` | 4-bit grayscale | decode to `Gray8` | PASS |
| `basn3p01.png` | 1-bit indexed | expand to `Rgb24` | PASS |
| `basn3p02.png` | 2-bit indexed | expand to `Rgb24` | PASS |
| `basn3p04.png` | 4-bit indexed | expand to `Rgb24` | PASS |
| `basn0g16.png` | 16-bit grayscale | deterministic downconversion to current packed frame model | PASS |
| `basn2c16.png` | 16-bit RGB | deterministic downconversion to current packed frame model | PASS |
| `basn4a16.png` | 16-bit gray+alpha | preserve alpha during conversion | PASS |
| `basn6a16.png` | 16-bit RGBA | preserve alpha during conversion | PASS |
| `basn3p08-trns.png` | indexed `tRNS` | remain `Rgba32`; regression guard | PASS |
| `basi0g01.png` vs non-interlaced reference | upstream Adam7 | pixel-identical decoded frame | PASS |
| generated full 15-format Adam7 matrix | every legal basic color/depth combination | exact expected pixel buffer | PASS |

## Adversarial coverage

- [x] packed rows whose width is not byte-aligned
- [x] unused tail bits never become pixels
- [x] truncated packed rows
- [x] decompressed data shorter than the exact pass/row budget
- [x] illegal PNG bit-depth/color-type combinations rejected
- [x] palette indices outside `PLTE` rejected
- [x] `PLTE` entry count above the bit-depth addressable range rejected
- [x] `tRNS` transparent-key comparison occurs before 16-bit downconversion
- [x] awkward Adam7 dimensions with empty passes
- [x] truncated Adam7 pass payload rejected
- [x] all five PNG filters exercised inside Adam7 passes
- [x] decompression output limits remain enforced by repository-owned zlib/DEFLATE
- [x] malformed inputs return errors rather than panicking
- [x] `ffmpeg` Adam7 output is pixel-equivalent to the upstream non-interlaced reference
- [x] malformed Adam7 input does not create `ffmpeg` output
- [x] `ffprobe` reports real Adam7 stream metadata
- [x] malformed Adam7 input does not emit fake `ffprobe` stream metadata

## Release gate

The validated clean head must satisfy all of the following before merge:

1. [x] `cargo test --workspace --all-targets` passes on Linux.
2. [x] `cargo test --workspace --all-targets` passes on macOS.
3. [x] `cargo test --workspace --all-targets` passes on Windows.
4. [x] `cargo build --workspace --release` passes on the Linux quality lane.
5. [x] `cargo clippy --workspace --all-targets` exits successfully under the repository's current lint policy.
6. [x] `cargo fmt --all --check` passes under Rust 1.98.0.
7. [x] Cargo metadata contains no registry or git dependency.
8. [x] Existing WAVE/PCM/Netpbm/BMP/TGA/PNG CLI regression tests remain green.
9. [x] Temporary patch/formatter workflows are absent from the clean head.

### Lint-policy note

An earlier draft of this matrix incorrectly stated that CI already enforced `cargo clippy ... -- -D warnings`. It does not: the repository currently enables broad Clippy lint groups and carries known warning debt outside this PNG slice. The release gate therefore records the command CI actually enforces instead of falsely claiming a warning-zero policy. A future repository-wide lint-hardening change can promote warnings to errors after that debt is removed.

## Scope boundary

This matrix completes the **PNG decode depth/interlace slice**, not complete PNG or FFmpeg parity. Current known PNG work still includes broader encoder parity, compression strategy/performance parity, metadata/ancillary chunk behavior, and exact FFmpeg option/diagnostic parity. Those remain explicitly incomplete.
