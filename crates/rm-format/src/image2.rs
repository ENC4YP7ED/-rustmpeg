use std::fs;
use std::path::{Path, PathBuf};

use rm_core::{MediaError, Result};

const MAX_PATTERN_WIDTH: usize = 18;
const DEFAULT_SCAN_LIMIT: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImagePattern {
    Single(PathBuf),
    Numbered(String),
}

impl ImagePattern {
    pub fn parse(path: &Path) -> Result<Self> {
        let Some(text) = path.to_str() else {
            return Ok(Self::Single(path.to_path_buf()));
        };
        let placeholders = count_placeholders(text)?;
        match placeholders {
            0 => Ok(Self::Single(path.to_path_buf())),
            1 => Ok(Self::Numbered(text.to_owned())),
            _ => Err(MediaError::invalid_argument(
                "image2 pattern may contain only one numeric placeholder",
            )),
        }
    }

    #[must_use]
    pub const fn is_sequence(&self) -> bool {
        matches!(self, Self::Numbered(_))
    }

    pub fn path_for(&self, number: i64) -> Result<PathBuf> {
        match self {
            Self::Single(path) => Ok(path.clone()),
            Self::Numbered(template) => Ok(PathBuf::from(render_pattern(template, number)?)),
        }
    }

    pub fn collect_existing(&self, start_number: i64, limit: Option<usize>) -> Result<Vec<PathBuf>> {
        self.collect_existing_in_range(start_number, 1, limit)
    }

    pub fn collect_existing_in_range(
        &self,
        start_number: i64,
        start_number_range: usize,
        limit: Option<usize>,
    ) -> Result<Vec<PathBuf>> {
        let limit = validate_limit(limit)?;
        if start_number_range == 0 || start_number_range > DEFAULT_SCAN_LIMIT {
            return Err(MediaError::invalid_argument(format!(
                "image2 start-number range must be in 1..={DEFAULT_SCAN_LIMIT}"
            )));
        }

        match self {
            Self::Single(path) => {
                if !path.is_file() {
                    return Err(MediaError::invalid_argument(format!(
                        "input '{}' does not exist or is not a file",
                        path.display()
                    )));
                }
                Ok(vec![path.clone()])
            }
            Self::Numbered(_) => {
                let first = self.find_first(start_number, start_number_range)?;
                let mut paths = Vec::new();
                for offset in 0..limit {
                    let offset = i64::try_from(offset)
                        .map_err(|_| MediaError::overflow("image2 sequence offset exceeds i64"))?;
                    let number = first
                        .checked_add(offset)
                        .ok_or_else(|| MediaError::overflow("image2 frame number overflow"))?;
                    let path = self.path_for(number)?;
                    match fs::metadata(&path) {
                        Ok(metadata) if metadata.is_file() => paths.push(path),
                        Ok(_) => break,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                        Err(error) => return Err(error.into()),
                    }
                }
                Ok(paths)
            }
        }
    }

    fn find_first(&self, start_number: i64, range: usize) -> Result<i64> {
        for offset in 0..range {
            let offset = i64::try_from(offset)
                .map_err(|_| MediaError::overflow("image2 start offset exceeds i64"))?;
            let number = start_number
                .checked_add(offset)
                .ok_or_else(|| MediaError::overflow("image2 start number overflow"))?;
            let path = self.path_for(number)?;
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => return Ok(number),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(MediaError::invalid_argument(format!(
            "no image2 frame found in start range {start_number}..{}",
            start_number
                .checked_add(i64::try_from(range - 1).unwrap_or(i64::MAX))
                .unwrap_or(i64::MAX)
        )))
    }
}

fn validate_limit(limit: Option<usize>) -> Result<usize> {
    let limit = limit.unwrap_or(DEFAULT_SCAN_LIMIT);
    if limit == 0 || limit > DEFAULT_SCAN_LIMIT {
        return Err(MediaError::invalid_argument(format!(
            "image2 scan limit must be in 1..={DEFAULT_SCAN_LIMIT}"
        )));
    }
    Ok(limit)
}

fn count_placeholders(template: &str) -> Result<usize> {
    let bytes = template.as_bytes();
    let mut index = 0;
    let mut count = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        index += 1;
        if index >= bytes.len() {
            return Err(MediaError::invalid_argument(
                "image2 pattern ends with an incomplete '%' escape",
            ));
        }
        if bytes[index] == b'%' {
            index += 1;
            continue;
        }
        parse_placeholder(bytes, &mut index)?;
        count += 1;
    }
    Ok(count)
}

fn parse_placeholder(bytes: &[u8], index: &mut usize) -> Result<usize> {
    let mut width = 0_usize;
    if bytes[*index] == b'0' {
        *index += 1;
        let start = *index;
        while *index < bytes.len() && bytes[*index].is_ascii_digit() {
            width = width
                .checked_mul(10)
                .and_then(|value| value.checked_add(usize::from(bytes[*index] - b'0')))
                .ok_or_else(|| MediaError::overflow("image2 pattern width overflow"))?;
            *index += 1;
        }
        if *index == start || width == 0 || width > MAX_PATTERN_WIDTH {
            return Err(MediaError::invalid_argument(format!(
                "image2 zero-pad width must be in 1..={MAX_PATTERN_WIDTH}"
            )));
        }
    }
    if *index >= bytes.len() || bytes[*index] != b'd' {
        return Err(MediaError::invalid_argument(
            "image2 supports only %d and %0Nd numeric placeholders",
        ));
    }
    *index += 1;
    Ok(width)
}

fn render_pattern(template: &str, number: i64) -> Result<String> {
    let bytes = template.as_bytes();
    let mut output = String::with_capacity(template.len().saturating_add(24));
    let mut index = 0;
    let mut rendered = false;

    while index < bytes.len() {
        if bytes[index] != b'%' {
            let ch = template[index..]
                .chars()
                .next()
                .ok_or_else(|| MediaError::invalid_argument("invalid UTF-8 pattern boundary"))?;
            output.push(ch);
            index += ch.len_utf8();
            continue;
        }

        index += 1;
        if bytes.get(index) == Some(&b'%') {
            output.push('%');
            index += 1;
            continue;
        }
        if rendered {
            return Err(MediaError::invalid_argument(
                "image2 pattern may contain only one numeric placeholder",
            ));
        }
        let width = parse_placeholder(bytes, &mut index)?;
        if width == 0 {
            output.push_str(&number.to_string());
        } else if number < 0 {
            let magnitude = number.unsigned_abs();
            let digit_width = width.saturating_sub(1);
            output.push('-');
            output.push_str(&format!("{magnitude:0digit_width$}"));
        } else {
            output.push_str(&format!("{number:0width$}"));
        }
        rendered = true;
    }

    if !rendered {
        return Err(MediaError::invalid_argument(
            "image2 numbered pattern has no numeric placeholder",
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "rustmpeg-image2-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn patterns_expand_plain_and_zero_padded_numbers() {
        let plain = ImagePattern::parse(Path::new("frame-%d.ppm")).unwrap();
        assert_eq!(plain.path_for(12).unwrap(), PathBuf::from("frame-12.ppm"));

        let padded = ImagePattern::parse(Path::new("frame-%06d.ppm")).unwrap();
        assert_eq!(
            padded.path_for(12).unwrap(),
            PathBuf::from("frame-000012.ppm")
        );
    }

    #[test]
    fn escaped_percent_is_preserved() {
        let pattern = ImagePattern::parse(Path::new("100%%-%03d.pgm")).unwrap();
        assert_eq!(pattern.path_for(7).unwrap(), PathBuf::from("100%-007.pgm"));
    }

    #[test]
    fn malformed_or_multiple_placeholders_are_rejected() {
        assert!(ImagePattern::parse(Path::new("frame-%s.ppm")).is_err());
        assert!(ImagePattern::parse(Path::new("frame-%00d.ppm")).is_err());
        assert!(ImagePattern::parse(Path::new("%d-%d.ppm")).is_err());
        assert!(ImagePattern::parse(Path::new("frame-%99d.ppm")).is_err());
    }

    #[test]
    fn collection_stops_at_first_missing_frame() {
        let dir = temp_dir("collect");
        fs::write(dir.join("f-001.ppm"), b"x").unwrap();
        fs::write(dir.join("f-002.ppm"), b"x").unwrap();
        fs::write(dir.join("f-004.ppm"), b"x").unwrap();
        let pattern = ImagePattern::parse(&dir.join("f-%03d.ppm")).unwrap();
        let paths = pattern.collect_existing(1, Some(10)).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths[0].ends_with("f-001.ppm"));
        assert!(paths[1].ends_with("f-002.ppm"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn start_number_range_finds_first_available_frame() {
        let dir = temp_dir("range");
        fs::write(dir.join("f-003.ppm"), b"x").unwrap();
        fs::write(dir.join("f-004.ppm"), b"x").unwrap();
        let pattern = ImagePattern::parse(&dir.join("f-%03d.ppm")).unwrap();
        let paths = pattern
            .collect_existing_in_range(0, 5, Some(10))
            .unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths[0].ends_with("f-003.ppm"));
        fs::remove_dir_all(dir).unwrap();
    }
}
