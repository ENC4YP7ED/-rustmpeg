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

const BASN0G08: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAAAAABWESUoAAAABGdBTUEAAYag\nMeiWXwAAAEFJREFUOI1jZGAkABQIyLMMBQWMDwgp+PcfP2B5MBwUMMoRkGdk\nonlcDAYFjI/wyv7/z/iH5nExGBQwyuCVZWQEAFDl/nFxqQskAAAAAElFTkSu\nQmCC\n";
const BASN2C08: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAABGdBTUEAAYag\nMeiWXwAAAEhJREFUSInt1cEJADAMAkCF7JH9t3ITO0Qr9KH4zuErtA0EO4AK\nFPgcoO3kfUx4QIECD0qHH8KEBxQo8KB0OCOpQIG7cHejwAGCsflePteTmAAA\nAABJRU5ErkJggg==\n";
const BASN4A08: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAQAAADZc7J/AAAABGdBTUEAAYag\nMeiWXwAAADVJREFUSIlj/M/AwAGFnGg0MSKcLN8ZKAMsP4a+AaNhMBoGVDFg\nNBBHw4AqBowG4mgYUMMAAN8qIH2gv3FYAAAAAElFTkSuQmCC\n";
const BASN6A08: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAYAAABzenr0AAAABGdBTUEAAYag\nMeiWXwAAAG9JREFUWIXt1jEKgDAMRuEnZGhPofc/VQSPIcTdxUV4HVLoUCj8\nH00o2YoBMF57fpz/ujODHXUFRwPKBqj5DVigB041HiJ9gFyCVOMbsEIPXNwu\nAHkgiJL/4qABNqB7QAeUPBAE2QAZUDZAfwEb8ABSIBqc1o/pZwAAAABJRU5E\nrkJggg==\n";
const BASN3P08_TRNS: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAMAAABEpIrGAAAABGdBTUEAAYag\nMeiWXwAAAwBQTFRF/wMH/wQH/wkH2Q4H/w4HAhYT/xoH/x8HCiUOsyUG/ioH\n/y0HGS4JADD+ADD/ADH/ADP+ADT//zUHADb8/jkH+zkH9zsHADs9AD7/jj8F\nAD/6/z8H/UQHAEn/AEn2/0sHUlUJ/1UHAFn/AFvt/14H8WQHAGX//WkHAGvf\n/2oHAW5f/3MHAHX//3wHdn4KAIL6AIT/AIbP/4YHAIj524wGAIz8AIz/AY6I\n/48H85YHxpgHpZkHAJ3//54HRp8EAKD7y6MGAKPvAaSy/6YHAamlAar/6KwG\n/68HubCDAbPhvLN2x7QGAbb/Abj5/7gHz7pHwbsG/b8H2sEwAcGdAcT0AcT+\nMMcDpMcF3MoG/csHAczM+9EH59AYAdL+AtOSAdScAdX87dsPAdrwpdwFAd36\n+d0Gkt4EAeC4AuCb9OEK+eMHAuWFwOQGJeYD9uYHj+gE9OkIAuyLAezjAe7u\nZfEEAfHaAfDop/AFG/MCfvMEAvZxhfgFFvoBAvnblPoFAvrHt/wFsPwFAvzT\nAvy+pPsFDP6AwP0FpP0FGv5VDv4Bhf0FBP20xP0FAv3GA/9bA/9Quv8FCf8C\nA/92Cf8DCv8BA/9MA/9WA/9SDf8BA/8xA/9lPf8ggf8Fsf8FA/8llf8FB/8G\nwP8FAv+DA/9iVf8LAv+jAv+VBP8XBv8MA/9DoP8Fd/8GZv8I/////v7+/v7+\n/Pz8/Pz8+vr6+vr6+Pj4+Pj49/f39fX19fX18/Pz8/Pz8fHx8fHx7+/v7u7u\n7u7u7Ozs7Ozs6urq6urq6Ojo5+fn5+fn5eXl5eXl4+Pj4uLi4uLi4ODg4ODg\n3t7e3t7e3Nzc29vb29vb2dnZ2dnZ19fX1tbW1tbW1NTU1NTU0tLS0dHR0dHR\nz8/Pzc3Nzc3NzMzMzMzMysrKycnJycnJx8fHx8fHxcXFxMTExMTEwsLCwcHB\nwcHBv7+/v7+/vb29vLy8vLy8urq6ubm5ubm5t7e3tra2tra2tLS0srKysrKy\nsbGxsbGxr6+vrq6urq6u/+L2KQAAAK10Uk5Txbu+wroEur0Ew1S/BcGvo82W\nv9VYS0MIk7/cy1+X38cIz5zjx0GjYuLMDMqnyQtBsuTNSjtXshPJYxIOuMy4\nYBY948cWwWHF/jv9HMBmx/c6xvQebcq8IGDEPMvvyuYpz+13NdXRJTct5tbp\nXLnfMuY5fNkrhd1fxi/pY8Lda4qYkOKMhdysfdrEduGh3+vuyJuTkqzs7Je3\nlurY2dOX24S5kZPZipCJjpfZ2dXh8TtsAAAB9ElEQVQ4y03R72tScRTH8duD\nWyBU1NhltBT7HVKt2p0yW6RoiThDcWIp0vSqxVqrqbf1A/NaOkktXCZBVM5y\nRaM/snPO9/u9+nr6eT86R5LtdkVRLNwBQTLJbrcyUVgsB8khbhYDKpTp6ZPM\ncTQrUOA+hy4IZ5lThAKPqqo8WASXmesEAw8GqrooBAIBn893k4HAEwqFIpHI\nHeE+ikajd1OpTCYzDu5xnU4Hi1R+beMZgCDEgnVzxySfXxPBDdg1TYusk2bN\ngH17OBx2N8wgkYBAw3mr2axhAUG3+x3sY5AoFApPNW0LvGk26/WaYWzDvg8G\ngwELXoLXqAXqdcP4ZoKgUGZE0Gr9M41GknylXB4XsD5hRtxEsEt6vR7uLwRJ\ntk+sFOzsfAbvOUl2OByb6I/wg+sTCGKxR5vM3wn9vghc4+AX9wV8BR+BJPsh\nEMXeT/QJNBqNV4SCB7zYI79Rm3sMgR+Ch6RYLFar78gHpt1mAYzJZDILQVHX\nqyBdKlUqlbeAApqT4ZVslgpdT6ehKJWeAwpiwSAGWABdX4YgvZrL5XjgD5Iw\nFCvYLKN4PL6KDQX+oNfrnV+4RcJOp3Np6Ta6BlgwB+YXyCV0FV0kcEm2s8Jq\nPQ3OgzMcBC7XHC9miBVNnUA2mw2eJYKZY+DoEXB4bOo/eOFfg6HM/SUAAAAA\nSUVORK5CYII=\n";
const BASN3P08: &str = "iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAMAAABEpIrGAAAABGdBTUEAAYag\nMeiWXwAAAwBQTFRFIkQA9f/td/93y///EQoAOncAIiL//xH/EQAAIiIA/6xV\nZv9m/2Zm/wH/IhIA3P//zP+ZRET/AFVVIgAAy8v/REQAVf9Vy8sAMxoA/+zc\n7f//5P/L/9zcRP9EZmb/MwAARCIA7e3/ZmYA/6RE//+q7e0AAMvL/v///f/+\n//8BM/8zVSoAAQH/iIj/AKqqAQEARAAAiIgA/+TLulsAIv8iZjIA//+Zqqr/\nVQAAqqoAy2MAEf8R1P+qdzoA/0RE3GsAZgAAAf8BiEIA7P/ca9wA/9y6ADMz\nAO0A7XMA//+ImUoAEf//dwAA/4MB/7q6/nsA//7/AMsA/5mZIv//iAAA//93\nAIiI/9z/GjMAAACqM///AJkAmQAAAAABMmYA/7r/RP///6r/AHcAAP7+qgAA\nSpkA//9m/yIiAACZi/8RVf///wEB/4j/AFUAABER///+//3+pP9EZv///2b/\nADMA//9V/3d3AACI/0T/ABEAd///AGZm///tAAEA//XtERH///9E/yL//+3t\nEREAiP//AAB3k/8iANzcMzP//gD+urr/mf//MzMAY8sAuroArP9V///c//8z\ne/4A7QDtVVX/qv//3Nz/VVUAAABm3NwA3ADcg/8Bd3f//v7////L/1VVd3cA\n/v4AywDLAAD+AQIAAQAAEiIA//8iAEREm/8z/9SqAABVmZn/mZkAugC6KlUA\n/8vLtP9m/5sz//+6qgCqQogAU6oA/6qqAADtALq6//8RAP4AAABEAJmZmQCZ\n/8yZugAAiACIANwA/5MiAADc/v/+qlMAdwB3AgEAywAAAAAz/+3/ALoA/zMz\n7f/t/8SIvP93AKoAZgBmACIi3AAA/8v/3P/c/4sRAADLAAEBVQBVAIgAAAAi\nAf//y//L7QAA/4iIRABEW7oA/7x3/5n/AGYAuv+6AHd3c+0A/gAAMwAzAAC6\n/3f/AEQAqv+q//7+AAARIgAixP+IAO3tmf+Z/1X/ACIA/7RmEQARChEA/xER\n3P+6uv//iP+IAQAB/zP/uY7TYgAAAbFJREFUOI0NwQcACAQQAMBHqIxIZCs7\nMwlla1hlZ+8VitCw9yoqNGiYDatsyt6jjIadlVkysve+u5jC9xTmV/qyl6bc\nJR7kAQZzg568xXmuE2lIyUNM5So7OMAFIhvp+YgGvEtFNnOKeJonSEvwP9NZ\nzhHiOfLzBXPoxKP8yD6iPMXITjP+oTdfsp14lTJMJjGtOMFQfiFe4wWK8BP7\nqUd31hBNqMos2tKYFbRnJdGGjTzPz2yjEA1ZSKymKCM5ylaWcJrZxCZK8jgf\nU4vc/MW3xE7K8RUvsZb3Wc/XxCEqk4v/qMQlFvMZcZIafMOnLKM13zGceJNq\nPMU4KnCQAqQgbrKHpXSgFK/Qn6REO9YxjWE8Sx2SMJD4jfl8wgzy0YgPuEeU\nJQcD6EoWWpCaHsQkHuY9RpGON/icK0RyrvE680jG22TlHaIbx6jLnySkF+M5\nQxzmD6pwkTsMoSAdidqsojipuMyHzOQ4sYgfyElpzjKGErQkqvMyC7jFv9xm\nBM2JuTzDRDLxN4l4jF1EZjIwmhfZzSOMpT4xiH70IQG/k5En2UKcowudycsG\n8jCBmtwHgRv+EAGUsVIAAAAASUVORK5CYII=\n";

#[test]
fn pngsuite_existing_8_bit_gray_and_rgb_paths_decode() {
    let gray = decode_png(&decode_base64(BASN0G08)).unwrap();
    assert_eq!(
        (gray.width, gray.height, gray.format),
        (32, 32, PixelFormat::Gray8)
    );

    let rgb = decode_png(&decode_base64(BASN2C08)).unwrap();
    assert_eq!(
        (rgb.width, rgb.height, rgb.format),
        (32, 32, PixelFormat::Rgb24)
    );
}

#[test]
fn pngsuite_8_bit_alpha_paths_decode() {
    let gray_alpha = decode_png(&decode_base64(BASN4A08)).unwrap();
    assert_eq!(
        (gray_alpha.width, gray_alpha.height, gray_alpha.format),
        (32, 32, PixelFormat::GrayAlpha8)
    );

    let rgba = decode_png(&decode_base64(BASN6A08)).unwrap();
    assert_eq!(
        (rgba.width, rgba.height, rgba.format),
        (32, 32, PixelFormat::Rgba32)
    );
}

#[test]
fn pngsuite_8_bit_palette_decodes_to_rgb() {
    let frame = decode_png(&decode_base64(BASN3P08)).unwrap();
    assert_eq!(
        (frame.width, frame.height, frame.format),
        (32, 32, PixelFormat::Rgb24)
    );
}

#[test]
fn pngsuite_indexed_transparency_decodes_to_rgba() {
    let frame = decode_png(&decode_base64(BASN3P08_TRNS)).unwrap();
    assert_eq!(
        (frame.width, frame.height, frame.format),
        (32, 32, PixelFormat::Rgba32)
    );
}
