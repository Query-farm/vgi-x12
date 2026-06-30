//! The [`Segment`] row model and the delimiter-driven segment/element splitter.
//!
//! A segment is a list of fields separated by the element separator; field 0 is
//! the segment ID (`ISA`, `GS`, `CLP`, `NM1`, …) and fields 1.. are the data
//! elements. Splitting is delimiter-agnostic: the four sniffed bytes are treated
//! as opaque. For EDIFACT, the release byte un-escapes a following delimiter so
//! it is data (`?+` → literal `+`).

use crate::delimiters::Delimiters;

/// One parsed segment: the raw field list plus its byte offset in the source.
///
/// `fields[0]` is the segment ID; `fields[1]` is X12 element position 1, so the
/// shaped extractors read `CLP04` as [`Segment::elem`]`(4)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// All fields: index 0 is the segment ID, indices 1.. are data elements.
    pub fields: Vec<String>,
    /// Start offset of this segment in the source buffer (debug / replay).
    pub byte_offset: u64,
}

impl Segment {
    /// The segment ID (`fields[0]`), or `""` for a degenerate empty segment.
    pub fn id(&self) -> &str {
        self.fields.first().map(String::as_str).unwrap_or("")
    }

    /// The X12 1-based data element at position `n` (so `n = 4` is `CLP04`),
    /// or `""` when the element is omitted / out of range.
    pub fn elem(&self, n: usize) -> &str {
        self.fields.get(n).map(String::as_str).unwrap_or("")
    }

    /// The `c`-th 1-based component (sub-element) of element `n`, split on the
    /// component separator (so `elem_comp(1, 2, sep)` is `SVC01-2`), or `""`.
    pub fn elem_comp(&self, n: usize, c: usize, component: u8) -> &str {
        let raw = self.elem(n);
        if raw.is_empty() || c == 0 {
            return "";
        }
        raw.split(component as char).nth(c - 1).unwrap_or("")
    }

    /// The data elements (everything after the segment ID) as string slices.
    pub fn data_elements(&self) -> &[String] {
        self.fields.get(1..).unwrap_or(&[])
    }
}

/// Split one raw segment slice (the bytes between two segment terminators, with
/// surrounding line-ending whitespace trimmed) into a [`Segment`] at
/// `byte_offset`, honoring the element separator and an optional release byte.
fn split_segment(raw: &[u8], delims: &Delimiters, byte_offset: u64) -> Segment {
    let fields = split_on(raw, delims.element, delims.release);
    Segment {
        fields,
        byte_offset,
    }
}

/// Split `raw` on `sep`, applying the optional `release` escape byte (EDIFACT):
/// a `release` byte makes the next byte literal data and is itself dropped.
fn split_on(raw: &[u8], sep: u8, release: Option<u8>) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let b = raw[i];
        if let Some(rel) = release {
            if b == rel && i + 1 < raw.len() {
                // The released byte is literal data; drop the release marker.
                cur.push(raw[i + 1]);
                i += 2;
                continue;
            }
        }
        if b == sep {
            out.push(String::from_utf8_lossy(&cur).into_owned());
            cur.clear();
        } else {
            cur.push(b);
        }
        i += 1;
    }
    out.push(String::from_utf8_lossy(&cur).into_owned());
    out
}

/// Split a repeated element value on the repetition separator, applying the
/// optional release byte. Returns the single element verbatim when `repetition`
/// is `None` or the separator does not occur.
pub fn split_repetitions(value: &str, delims: &Delimiters) -> Vec<String> {
    match delims.repetition {
        Some(rep) if value.as_bytes().contains(&rep) => {
            split_on(value.as_bytes(), rep, delims.release)
        }
        _ => vec![value.to_string()],
    }
}

/// Trim the line-ending bytes a sender may place after a segment terminator
/// (`\r`, `\n`) and any leading whitespace, so `~\r\n`-terminated and bare-`~`
/// terminated files parse identically — including files with mixed endings.
fn trim_segment(raw: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = raw.len();
    while start < end && matches!(raw[start], b'\r' | b'\n' | b' ' | b'\t') {
        start += 1;
    }
    while end > start && matches!(raw[end - 1], b'\r' | b'\n') {
        end -= 1;
    }
    &raw[start..end]
}

/// Explode `bytes` into segments using `delims`, recording each segment's byte
/// offset. Empty (whitespace-only) segments — e.g. the trailing fragment after
/// the final terminator — are skipped. Never panics on arbitrary input.
pub fn explode(bytes: &[u8], delims: &Delimiters) -> Vec<Segment> {
    let mut out = Vec::new();
    let seg = delims.segment;
    let release = delims.release;
    let mut field_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        // Honor the EDIFACT release byte so an escaped segment terminator inside
        // data does not prematurely end the segment.
        if let Some(rel) = release {
            if b == rel && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
        }
        if b == seg {
            let raw = &bytes[field_start..i];
            let trimmed = trim_segment(raw);
            if !trimmed.is_empty() {
                // Offset of the first non-trimmed byte.
                let lead = (trimmed.as_ptr() as usize) - (bytes.as_ptr() as usize);
                out.push(split_segment(trimmed, delims, lead as u64));
            }
            field_start = i + 1;
        }
        i += 1;
    }
    // A trailing fragment with no final terminator (e.g. truncated interchange)
    // is still surfaced so callers see what was received.
    if field_start < bytes.len() {
        let trimmed = trim_segment(&bytes[field_start..]);
        if !trimmed.is_empty() {
            let lead = (trimmed.as_ptr() as usize) - (bytes.as_ptr() as usize);
            out.push(split_segment(trimmed, delims, lead as u64));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explodes_canonical() {
        let d = Delimiters::x12_default();
        let segs = explode(b"GS*HP*S*R*20240101*1200*1*X*005010X221A1~ST*835*0001~", &d);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].id(), "GS");
        assert_eq!(segs[0].elem(1), "HP");
        assert_eq!(segs[1].id(), "ST");
        assert_eq!(segs[1].elem(1), "835");
        assert_eq!(segs[1].elem(2), "0001");
    }

    #[test]
    fn trims_crlf_and_mixed_endings() {
        let d = Delimiters::x12_default();
        let segs = explode(b"NM1*PR*2*ACME~\r\nN1*ST*DEST~N3*1 MAIN~\n", &d);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].id(), "NM1");
        assert_eq!(segs[2].id(), "N3");
        assert_eq!(segs[2].elem(1), "1 MAIN");
    }

    #[test]
    fn surfaces_truncated_trailing_segment() {
        let d = Delimiters::x12_default();
        // No final terminator on the last segment.
        let segs = explode(b"CLP*A*1*100~CLP*B*1*200", &d);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1].elem(1), "B");
    }

    #[test]
    fn components_and_repetitions() {
        let mut d = Delimiters::x12_default();
        d.component = b':';
        d.repetition = Some(b'^');
        let segs = explode(b"SVC*HC:99213:25*100*80*UN*1~", &d);
        let svc = &segs[0];
        assert_eq!(svc.elem(1), "HC:99213:25");
        assert_eq!(svc.elem_comp(1, 1, d.component), "HC");
        assert_eq!(svc.elem_comp(1, 2, d.component), "99213");
        assert_eq!(svc.elem_comp(1, 3, d.component), "25");

        let reps = split_repetitions("A^B^C", &d);
        assert_eq!(reps, vec!["A", "B", "C"]);
    }

    #[test]
    fn edifact_release_unescape() {
        let d = Delimiters::edifact_default();
        // `?+` is a literal plus inside the data, not an element separator.
        let segs = explode(b"FTX+AAA+++Price is 5?+ tax'", &d);
        let ftx = &segs[0];
        assert_eq!(ftx.id(), "FTX");
        assert_eq!(ftx.elem(4), "Price is 5+ tax");
    }
}
