# JPEG baseline/progressive pre-release matrix

JPEG is not advertised as complete until decoder, encoder, CLI, corpus, malformed-input, and cross-platform gates pass. Implementation must remain repository-owned and use no third-party Cargo dependencies or native codec libraries.

## Decoder acceptance

- [ ] SOI/EOI and marker framing
- [ ] DQT 8-bit and 16-bit quantization tables
- [ ] DHT canonical Huffman tables with malformed-tree rejection
- [ ] SOF0 baseline sequential DCT
- [ ] SOF2 progressive DCT
- [ ] SOS scan parsing and entropy byte stuffing
- [ ] restart intervals and RST0..RST7 sequencing
- [ ] grayscale
- [ ] YCbCr 4:4:4
- [ ] YCbCr 4:2:2
- [ ] YCbCr 4:2:0
- [ ] multi-scan baseline files
- [ ] progressive DC first/refinement
- [ ] progressive AC first/refinement and EOB runs
- [ ] integer reference IDCT with bounded coefficients
- [ ] edge MCU clipping for non-multiple dimensions
- [ ] EXIF/JFIF/Adobe marker tolerance without trusting metadata lengths
- [ ] CMYK/YCCK explicitly supported or explicitly rejected without misdecode

## Encoder acceptance

- [ ] baseline sequential grayscale
- [ ] baseline sequential YCbCr
- [ ] configurable quality/quantization scaling
- [ ] canonical Huffman emission
- [ ] byte stuffing
- [ ] correct edge padding
- [ ] decoder-independent validation by another standards implementation

## QA corpus

Use immutable upstream files from established JPEG test corpora and compare decoded pixels against independent reference output. Synthetic tests remain required for malformed lengths, invalid Huffman tables, restart sequencing, truncated entropy data, coefficient overflow, and progressive scan-order violations.

## Release gate

- [ ] Linux workspace tests
- [ ] Linux release build
- [ ] Clippy
- [ ] rustfmt
- [ ] zero third-party Cargo dependency gate
- [ ] macOS workspace tests
- [ ] Windows workspace tests
- [ ] independent external JPEG decoder accepts rustmpeg encoder output
- [ ] upstream baseline/progressive corpus passes
- [ ] ffmpeg and ffprobe process-level tests pass

No checklist item is marked complete from registry presence alone.
