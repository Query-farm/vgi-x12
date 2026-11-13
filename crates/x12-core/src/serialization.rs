//! Complete X12 segment rows and strict, incremental text serialization.
//!
//! The reader retains envelope and stray segments. The writer accepts complete
//! ASCII interchanges, preserves positional values, and checks public envelope
//! syntax only. Inter-segment formatting whitespace is not preserved.

use crate::{delimiters, segment, Delimiters, Segment};

/// A segment with the delimiter scope and envelope keys at its source position.
#[derive(Debug, Clone)]
pub struct SegmentRow {
    pub segment: Segment,
    pub delimiters: Delimiters,
    pub interchange_ctrl: String,
    pub group_ctrl: String,
    pub transaction_ctrl: String,
    pub transaction_type: String,
}

/// Read every segment, including ISA/GS/GE/IEA and malformed out-of-scope rows.
/// Re-sniff at each ISA boundary, so concatenated interchanges may use different
/// separators. Invalid text is handled as permissively as the existing readers.
pub fn read_segments(bytes: &[u8]) -> Vec<SegmentRow> {
    let Some(mut delims) = delimiters::sniff_x12(bytes) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let (mut interchange, mut group, mut transaction, mut transaction_type) =
        (String::new(), String::new(), String::new(), String::new());
    let mut pos = 0;
    while pos < bytes.len() {
        pos += delimiters::leading_ws(&bytes[pos..]);
        if pos == bytes.len() {
            break;
        }
        if let Some(next) = delimiters::sniff_x12(&bytes[pos..]) {
            delims = next;
        }
        let end = bytes[pos..]
            .iter()
            .position(|b| *b == delims.segment)
            .map_or(bytes.len(), |n| pos + n + 1);
        for mut seg in segment::explode(&bytes[pos..end], &delims) {
            seg.byte_offset += pos as u64;
            match seg.id() {
                "ISA" => {
                    interchange = seg.elem(13).to_string();
                    group.clear();
                    transaction.clear();
                    transaction_type.clear();
                }
                "GS" => {
                    group = seg.elem(6).to_string();
                    transaction.clear();
                    transaction_type.clear();
                }
                "ST" => {
                    transaction = seg.elem(2).to_string();
                    transaction_type = seg.elem(1).to_string();
                }
                _ => {}
            }
            let id = seg.id().to_string();
            rows.push(SegmentRow {
                segment: seg,
                delimiters: delims,
                interchange_ctrl: interchange.clone(),
                group_ctrl: group.clone(),
                transaction_ctrl: transaction.clone(),
                transaction_type: transaction_type.clone(),
            });
            match id.as_str() {
                "SE" => {
                    transaction.clear();
                    transaction_type.clear();
                }
                "GE" => {
                    group.clear();
                    transaction.clear();
                    transaction_type.clear();
                }
                "IEA" => {
                    interchange.clear();
                    group.clear();
                    transaction.clear();
                    transaction_type.clear();
                }
                _ => {}
            }
        }
        pos = end;
    }
    rows
}

/// Stateful validator/encoder. Keep one instance across all batches of a COPY.
/// Discard the instance and its output after any error.
#[derive(Default)]
pub struct X12Writer {
    recalculate_counts: bool,
    interchange: Option<String>,
    group: Option<String>,
    transaction: Option<String>,
    separators: Option<(u8, u8)>,
    groups: usize,
    transactions: usize,
    segments: usize,
}

type Result<T> = std::result::Result<T, String>;

impl X12Writer {
    pub fn new(recalculate_counts: bool) -> Self {
        Self {
            recalculate_counts,
            ..Self::default()
        }
    }

    /// Validate and encode one segment, including its terminator. Elements are
    /// unsplit: composite and repetition separators remain within their values.
    pub fn push(
        &mut self,
        id: &str,
        elements: &[String],
        element_separator: u8,
        segment_terminator: u8,
    ) -> Result<Vec<u8>> {
        if !(2..=3).contains(&id.len())
            || !id
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return Err("segment_id must contain 2 or 3 uppercase ASCII letters/digits".into());
        }
        if !separator(element_separator)
            || !(separator(segment_terminator) || matches!(segment_terminator, b'\n' | b'\r'))
            || element_separator == segment_terminator
        {
            return Err("invalid or conflicting X12 delimiters".into());
        }
        for value in elements {
            if value.bytes().any(|b| {
                !(b' '..=b'~').contains(&b) || b == element_separator || b == segment_terminator
            }) {
                return Err(format!(
                    "{id}: elements must be ASCII text without element/segment delimiters"
                ));
            }
        }
        if id != "ISA" && self.separators != Some((element_separator, segment_terminator)) {
            return Err(format!(
                "{id}: missing ISA or delimiters changed within an interchange"
            ));
        }
        let mut elements = elements.to_vec();
        match id {
            "ISA" => {
                if self.interchange.is_some() {
                    return Err("ISA before the previous interchange's IEA".into());
                }
                const WIDTHS: [usize; 16] = [2, 10, 2, 10, 2, 15, 2, 15, 6, 4, 1, 5, 9, 1, 1, 1];
                if elements.len() != WIDTHS.len()
                    || elements.iter().zip(WIDTHS).any(|(v, n)| v.len() != n)
                {
                    return Err(
                        "ISA requires 16 fixed-width elements (106 bytes including terminator)"
                            .into(),
                    );
                }
                if !elements[11].bytes().all(|b| b.is_ascii_digit())
                    || !elements[12].bytes().all(|b| b.is_ascii_digit())
                {
                    return Err("ISA12 and ISA13 must contain digits".into());
                }
                let component = elements[15].as_bytes()[0];
                let mut separators = vec![element_separator, segment_terminator, component];
                if elements[11].as_str() >= "00402" {
                    separators.push(elements[10].as_bytes()[0]);
                }
                for (i, b) in separators.iter().enumerate() {
                    if (i >= 2 && !separator(*b)) || separators[..i].contains(b) {
                        return Err(
                            "ISA declares invalid or conflicting component/repetition delimiters"
                                .into(),
                        );
                    }
                }
                self.interchange = Some(elements[12].clone());
                self.separators = Some((element_separator, segment_terminator));
                self.groups = 0;
            }
            "GS" => {
                if self.interchange.is_none() || self.group.is_some() || elements.len() != 8 {
                    return Err("GS requires an open ISA, no open group, and 8 elements".into());
                }
                require_control(&elements[5], "GS06")?;
                self.group = Some(elements[5].clone());
                self.transactions = 0;
            }
            "ST" => {
                if self.group.is_none()
                    || self.transaction.is_some()
                    || !(2..=3).contains(&elements.len())
                {
                    return Err(
                        "ST requires an open GS, no open transaction, and 2 or 3 elements".into(),
                    );
                }
                if elements[0].len() != 3 || !elements[0].bytes().all(|b| b.is_ascii_digit()) {
                    return Err("ST01 must contain 3 digits".into());
                }
                require_control(&elements[1], "ST02")?;
                self.transaction = Some(elements[1].clone());
                self.segments = 1;
            }
            "SE" => {
                check_trailer(&elements, self.transaction.as_deref(), "SE")?;
                self.segments += 1;
                check_count(
                    &mut elements[0],
                    self.segments,
                    self.recalculate_counts,
                    "SE01",
                )?;
                self.transaction = None;
                self.transactions += 1;
            }
            "GE" => {
                if self.transaction.is_some() || self.transactions == 0 {
                    return Err("GE requires completed ST/SE transactions".into());
                }
                check_trailer(&elements, self.group.as_deref(), "GE")?;
                check_count(
                    &mut elements[0],
                    self.transactions,
                    self.recalculate_counts,
                    "GE01",
                )?;
                self.group = None;
                self.groups += 1;
            }
            "IEA" => {
                if self.group.is_some() || self.transaction.is_some() || self.groups == 0 {
                    return Err("IEA requires completed GS/GE groups".into());
                }
                check_trailer(&elements, self.interchange.as_deref(), "IEA")?;
                check_count(
                    &mut elements[0],
                    self.groups,
                    self.recalculate_counts,
                    "IEA01",
                )?;
                self.interchange = None;
                self.separators = None;
            }
            "BIN" => {
                return Err("BIN binary payloads are not supported by the X12 text writer".into())
            }
            _ => {
                if self.transaction.is_none() {
                    return Err(format!("{id}: body segment outside ST/SE"));
                }
                self.segments += 1;
            }
        }
        let mut output = id.as_bytes().to_vec();
        for value in &elements {
            output.push(element_separator);
            output.extend_from_slice(value.as_bytes());
        }
        output.push(segment_terminator);
        Ok(output)
    }

    /// Reject truncated output. Zero rows produce an empty file.
    pub fn finish(&self) -> Result<()> {
        if self.interchange.is_some() || self.group.is_some() || self.transaction.is_some() {
            Err("incomplete X12 envelope: missing SE, GE, or IEA".into())
        } else {
            Ok(())
        }
    }
}

fn separator(b: u8) -> bool {
    b.is_ascii_punctuation()
}

fn require_control(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(format!("{name} control number must not be empty"))
    } else {
        Ok(())
    }
}

fn check_trailer(elements: &[String], control: Option<&str>, id: &str) -> Result<()> {
    if elements.len() != 2 || control.is_none() {
        return Err(format!(
            "{id} requires an open matching envelope and 2 elements"
        ));
    }
    if Some(elements[1].as_str()) != control {
        return Err(format!("{id} control number does not match its header"));
    }
    Ok(())
}

fn check_count(value: &mut String, expected: usize, repair: bool, name: &str) -> Result<()> {
    if repair {
        *value = expected.to_string();
    } else if value.is_empty()
        || !value.bytes().all(|b| b.is_ascii_digit())
        || value.parse::<usize>().ok() != Some(expected)
    {
        return Err(format!(
            "{name} count must be {expected}; use recalculate_counts to update trailer counts"
        ));
    }
    Ok(())
}
