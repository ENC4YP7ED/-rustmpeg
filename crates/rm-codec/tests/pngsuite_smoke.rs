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
        let c = if chunk[2] == b'=' { 0 } else { value(chunk[2]).unwrap() };
        let d = if chunk[3] == b'=' { 0 } else { value(chunk[3]).unwrap() };
        let word = (u32::from(a) << 18)
            | (u32::from(b) << 12)
            | (u32::from(c) << 6)
            | u32::from(d);
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

const BASN0G08: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAAAAABWESUoAAAABGdBTUEAAYag\nMeiWXwAAAEFJREFUOI1jZGAkABQIyLMMBQWMDwgp+PcfP2B5MBwUMMoRkGdk\nonlcDAYFjI/wyv7/z/iH5nExGBQwyuCVZWQEAFDl/nFxqQskAAAAAElFTkSu\nQmCC\n";
const BASN2C08: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAABGdBTUEAAYag\nMeiWXwAAAEhJREFUSInt1cEJADAMAkCF7JH9t3ITO0Qr9KH4zuErtA0EO4AK\nFPgcoO3kfUx4QIECD0qHH8KEBxQo8KB0OCOpQIG7cHejwAGCsflePteTmAAA\nAABJRU5ErkJggg==\n";
const BASN4A08: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAQAAADZc7J/AAAABGdBTUEAAYag\nMeiWXwAAADVJREFUSIlj/M/AwAGFnGg0MSKcLN8ZKAMsP4a+AaNhMBoGVDFg\nNBBHw4AqBowG4mgYUMMAAN8qIH2gv3FYAAAAAElFTkSuQmCC\n";
const BASN6A08: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAAABGdBTUEAAYag\nMeiWXwAAAG9JREFUWIXt1jEKgDAMRuEnZGhPofc/VQSPIcTdxUV4HVLoUCj8\nH00o2YoBMF57fpz/ujODHXUFRwPKBqj5DVigB041HiJ9gFyCVOMbsEIPXNwu\nAHkgiJL/4qABNqB7QAeUPBAE2QAZUDZAfwEb8ABSIBqc1o/pZwAAAABJRU5E\nrkJggg==\n";
const BASN3P08: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAMAAABEpIrGAAAABGdBTUEAAYag\nMeiWXwAAAwBQTFRFIkQA9f/td/93y///EQoAOncAIiL//xH/EQAAIiIA/6xV\nZv9m/2Zm/wH/IhIA3P//zP+ZRET/AFVVIgAAy8v/REQAVf9Vy8sAMxoA/+zc\n7f//5P/L/9zcRP9EZmb/MwAARCIA7e3/ZmYA/6RE//+q7e0AAMvL/v///f/+\n//8BM/8zVSoAAQH/iIj/AKqqAQEARAAAiIgA/+TLulsAIv8iZjIA//+Zqqr/\nVQAAqqoAy2MAEf8R1P+qdzoA/0RE3GsAZgAAAf8BiEIA7P/ca9wA/9y6ADMz\nAO0A7XMA//+ImUoAEf//dwAA/4MB/7q6/nsA//7/AMsA/5mZIv//iAAA//93\nAIiI/9z/GjMAAACqM///AJkAmQAAAAABMmYA/7r/RP///6r/AHcAAP7+qgAA\nSpkA//9m/yIiAACZi/8RVf///wEB/4j/AFUAABER///+//3+pP9EZv///2b/\nADMA//9V/3d3AACI/0T/ABEAd///AGZm///tAAEA//XtERH///9E/yL//+3t\nEREAiP//AAB3k/8iANzcMzP//gD+urr/mf//MzMAY8sAuroArP9V///c//8z\ne/4A7QDtVVX/qv//3Nz/VVUAAABm3NwA3ADcg/8Bd3f//v7////L/1VVd3cA\n/v4AywDLAAD+AQIAAQAAEiIA//8iAEREm/8z/9SqAABVmZn/mZkAugC6KlUA\n/8vLtP9m/5sz//+6qgCqQogAU6oA/6qqAADtALq6//8RAP4AAABEAJmZmQCZ\n/8yZugAAiACIANwA/5MiAADc/v/+qlMAdwB3AgEAywAAAAAz/+3/ALoA/zMz\n7f/t/8SIvP93AKoAZgBmACIi3AAA/8v/3P/c/4sRAADLAAEBVQBVAIgAAAAi\nAf//y//L7QAA/4iIRABEW7oA/7x3/5n/AGYAuv+6AHd3c+0A/gAAMwAzAAC6\n/3f/AEQAqv+q//7+AAARIgAixP+IAO3tmf+Z/1X/ACIA/7RmEQARChEA/xER\n3P+6uv//iP+IAQAB/zP/uY7TYgAAAbFJREFUOI0NwQcACAQQAMBHqIxIZCs7\nMwlla1hlZ+8VitCw9yoqNGiYDatsyt6jjIadlVkysve+u5jC9xTmV/qyl6bc\nJR7kAQZzg568xXmuE2lIyUNM5So7OMAFIhvp+YgGvEtFNnOKeJonSEvwP9NZ\nzhHiOfLzBXPoxKP8yD6iPMXITjP+oTdfsp14lTJMJjGtOMFQfiFe4wWK8BP7\nqUd31hBNqMos2tKYFbRnJdGGjTzPz2yjEA1ZSKymKCM5ylaWcJrZxCZK8jgf\nU4vc/MW3xE7K8RUvsZb3Wc/XxCEqk4v/qMQlFvMZcZIafMOnLKM13zGceJNq\nPMU4KnCQAqQgbrKHpXSgFK/Qn6REO9YxjWE8Sx2SMJD4jfl8wgzy0YgPuEeU\nJQcD6EoWWpCaHsQkHuY9RpGON/icK0RyrvE680jG22TlHaIbx6jLnySkF+M5\nQxzmD6pwkTsMoSAdidqsojipuMyHzOQ4sYgfyElpzjKGErQkqvMyC7jFv9xm\nBM2JuTzDRDLxN4l4jF1EZjIwmhfZzSOMpT4xiH70IQG/k5En2UKcowudycsG\n8jCBmtwHgRv+EAGUsVIAAAAASUVORK5CYII=\n";

#[test]
fn pngsuite_existing_8_bit_gray_and_rgb_paths_decode() {
    let gray = decode_png(&decode_base64(BASN0G08)).unwrap();
    assert_eq!((gray.width, gray.height, gray.format), (32, 32, PixelFormat::Gray8));

    let rgb = decode_png(&decode_base64(BASN2C08)).unwrap();
    assert_eq!((rgb.width, rgb.height, rgb.format), (32, 32, PixelFormat::Rgb24));
}

#[test]
fn pngsuite_vectors_expose_current_color_model_gaps() {
    for (name, vector) in [
        ("basn4a08 gray+alpha", BASN4A08),
        ("basn6a08 rgba", BASN6A08),
        ("basn3p08 palette", BASN3P08),
    ] {
        assert!(decode_png(&decode_base64(vector)).is_err(), "{name} unexpectedly decoded before support was implemented");
    }
}
