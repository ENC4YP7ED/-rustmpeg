use rm_codec::png::decode_png;
use rm_core::video::PixelFormat;

fn decode_base64(input: &str) -> Vec<u8> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let clean: Vec<u8> = input
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    assert!(clean.len().is_multiple_of(4));

    let mut output = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks_exact(4) {
        let a = value(chunk[0]).unwrap();
        let b = value(chunk[1]).unwrap();
        let c = if chunk[2] == b'=' {
            0
        } else {
            value(chunk[2]).unwrap()
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            value(chunk[3]).unwrap()
        };
        let word = (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        output.push((word >> 16) as u8);
        if chunk[2] != b'=' {
            output.push((word >> 8) as u8);
        }
        if chunk[3] != b'=' {
            output.push(word as u8);
        }
    }
    output
}

const BASN0G01: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgAQAAAABbAUdZAAAABGdBTUEAAYag\nMeiWXwAAAFtJREFUCJktzLEJAzAMBdHr0gSySiALejRvkBU8gsGNCmFFB1Hx\n4IovqurSpIRszqklUwbnUzRXEuIRsiG/SyY9G0JzJSVei9qynm9qyjBpLp0p\nYW7pbzBl8L8fEIdJL6WUeFsAAAAASUVORK5CYII=\n";
const BASN0G02: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgAgAAAAAcoT2JAAAABGdBTUEAAYag\nMeiWXwAAAB9JREFUGJVjYAhd9R+M8TCIUMIAU4aPATMJH2OQuQcAvUl/gYeL\nUAMAAAAASUVORK5CYII=\n";
const BASN0G04: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgBAAAAACT4cgpAAAABGdBTUEAAYag\nMeiWXwAAAEhJREFUKJFjYGAQFFRSMjZ2cQkNTUsrL2cgQwCV29FBjgAqd+ZM\ncgRQuatWkSOAyt29mxwBVO6ZM+QIoHLv3iVHAJX77h0ZAgAfFO4BXy1pQQAA\nAABJRU5ErkJggg==\n";
const BASN3P01: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgAQMAAABJtOi3AAAABGdBTUEAAYag\nMeiWXwAAAAZQTFRF7v8iImb/bBrSJgAAABVJREFUCJlj4AcCBjTiAxCgEwOk\nDgC7Hz/BkMXMgwAAAABJRU5ErkJggg==\n";
const BASN3P02: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgAgMAAAAOFJJnAAAABGdBTUEAAYag\nMeiWXwAAAANzQklUAQEBfC53ggAAAAxQTFRFAP8A/wAA//8AAAD/ZT8rugAA\nACJJREFUGJVj+B+6igGEGfAw8MnBGKugLHwMqNL/+BiDzD0AvUl/gW0qyvsA\nAAAASUVORK5CYII=\n";
const BASN3P04: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgBAMAAACBVGfHAAAABGdBTUEAAYag\nMeiWXwAAAANzQklUBAQEd/i1owAAAC1QTFRFIgD/AP//iAD/Iv8AAJn//2YA\n3QD/d/8A/wAAAP+Z3f8A/wC7/7sAAET/AP9E0rBJvQAAAEdJREFUKJFj6OgI\nDT1zZtWq8nJj43fvZs5kIEMAlSsoSI4AKtfFhRwBVO7du+QIoHEZyBFA5Sop\nkSOAyk1LI0cAlbt7NxkCAODE6tENZY/AAAAAAElFTkSuQmCC\n";

#[test]
fn pngsuite_subbyte_grayscale_decodes_to_gray8() {
    for (name, encoded) in [
        ("basn0g01", BASN0G01),
        ("basn0g02", BASN0G02),
        ("basn0g04", BASN0G04),
    ] {
        let frame = decode_png(&decode_base64(encoded)).unwrap_or_else(|error| {
            panic!("{name} failed to decode: {error}");
        });
        assert_eq!((frame.width, frame.height), (32, 32), "{name}");
        assert_eq!(frame.format, PixelFormat::Gray8, "{name}");
        assert_eq!(frame.data.len(), 32 * 32, "{name}");
    }
}

#[test]
fn pngsuite_subbyte_indexed_decodes_to_rgb24() {
    for (name, encoded) in [
        ("basn3p01", BASN3P01),
        ("basn3p02", BASN3P02),
        ("basn3p04", BASN3P04),
    ] {
        let frame = decode_png(&decode_base64(encoded)).unwrap_or_else(|error| {
            panic!("{name} failed to decode: {error}");
        });
        assert_eq!((frame.width, frame.height), (32, 32), "{name}");
        assert_eq!(frame.format, PixelFormat::Rgb24, "{name}");
        assert_eq!(frame.data.len(), 32 * 32 * 3, "{name}");
    }
}
