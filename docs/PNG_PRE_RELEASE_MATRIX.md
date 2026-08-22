# PNG depth and Adam7 pre-release matrix

This document is an executable-release contract for the next PNG slice. A capability is not marked complete because synthetic unit tests pass; it must satisfy the upstream corpus and malformed-input checks listed here.

## Upstream acceptance corpus

Primary corpus: PngSuite as mirrored by `pnggroup/libpng` and `golang/go`.

| Vector | Capability | Required result |
| --- | --- | --- |
| `basn0g01.png` | 1-bit grayscale | decode to `Gray8` |
| `basn0g02.png` | 2-bit grayscale | decode to `Gray8` |
| `basn0g04.png` | 4-bit grayscale | decode to `Gray8` |
| `basn3p01.png` | 1-bit indexed | expand to `Rgb24` |
| `basn3p02.png` | 2-bit indexed | expand to `Rgb24` |
| `basn3p04.png` | 4-bit indexed | expand to `Rgb24` |
| `basn0g16.png` | 16-bit grayscale | decode deterministically to current packed frame model |
| `basn2c16.png` | 16-bit RGB | decode deterministically to current packed frame model |
| `basn4a16.png` | 16-bit gray+alpha | preserve alpha during conversion |
| `basn6a16.png` | 16-bit RGBA | preserve alpha during conversion |
| `basn3p08-trns.png` | indexed `tRNS` | remain `Rgba32`; regression guard |
| interlaced `basi*` counterparts | Adam7 | pixel-identical to matching non-interlaced reference after decode |

## Required adversarial coverage

- packed rows whose width is not byte-aligned
- unused tail bits must never become pixels
- truncated packed rows
- decompressed data shorter or longer than the exact pass/row budget
- illegal PNG bit-depth/color-type combinations
- palette indices outside `PLTE`
- `PLTE` entry count above the bit-depth addressable range
- `tRNS` transparent-key comparisons performed before 16-bit downconversion
- malformed Adam7 pass dimensions and truncated pass payloads
- all five PNG filters inside Adam7 passes
- decompression output limits remain enforced for interlaced images
- no panic on malformed input

## Release gate

Before merge:

1. `cargo test --workspace --all-targets` passes on Linux, macOS, and Windows.
2. `cargo build --workspace --all-targets --release` passes.
3. `cargo clippy --workspace --all-targets -- -D warnings` passes.
4. `cargo fmt --all -- --check` passes under Rust 1.98.0.
5. Cargo metadata contains no registry or git dependency.
6. Every capability above has at least one upstream corpus acceptance test.
7. Existing WAVE/PCM/Netpbm/BMP/TGA/PNG CLI regression tests remain green.

No item is marked complete until the code and tests satisfy this matrix.
