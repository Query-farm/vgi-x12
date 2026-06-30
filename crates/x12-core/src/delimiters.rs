//! Delimiter discovery — the only thing in X12 that is fixed-position.
//!
//! An X12 interchange MUST begin with the literal `ISA`, and the ISA segment is
//! **exactly 106 bytes** of fixed-width fields. That is what makes delimiter
//! discovery deterministic rather than heuristic: the element separator is the
//! byte immediately after `ISA`, ISA11 (the repetition separator) sits at a
//! fixed offset, ISA16 (the component separator) is the last data element, and
//! the segment terminator is the byte right after ISA16.
//!
//! EDIFACT carries its delimiters in the optional 9-byte `UNA` service-string
//! advice (with documented defaults when `UNA` is absent), including a
//! release/escape byte that X12 has no equivalent for.

use serde::{Deserialize, Serialize};

/// Which EDI family an interchange belongs to, decided by the magic prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// ANSI ASC X12 — begins with `ISA`.
    X12,
    /// UN/EDIFACT — begins with `UNA` or `UNB`.
    Edifact,
    /// Neither magic prefix was found.
    Unknown,
}

/// The set of delimiter bytes governing one interchange.
///
/// For X12, `release` is always `None` (X12 has no escape character) and
/// `decimal` is unused. For EDIFACT, `repetition` is `None` and `release` /
/// `decimal` are meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delimiters {
    /// Element separator (X12 ISA byte 4 / EDIFACT UNA pos 2).
    pub element: u8,
    /// Segment terminator (X12 byte 106 / EDIFACT UNA pos 6).
    pub segment: u8,
    /// Component (sub-element) separator (X12 ISA16 / EDIFACT UNA pos 1).
    pub component: u8,
    /// Repetition separator (X12 ISA11 when it is a real char, not the `U`
    /// version-4010 placeholder). `None` when repetition is not in use.
    pub repetition: Option<u8>,
    /// Release / escape byte (EDIFACT UNA pos 4 only; `None` for X12).
    pub release: Option<u8>,
    /// Decimal-notation byte (EDIFACT UNA pos 3; informational, not used by the
    /// syntactic split). Defaults to `b'.'`.
    pub decimal: u8,
}

impl Delimiters {
    /// The canonical X12 delimiter set (`*` element, `~` segment, `:` component,
    /// `^` repetition). Used only as a fallback for degenerate input; real
    /// interchanges are always sniffed.
    pub fn x12_default() -> Self {
        Delimiters {
            element: b'*',
            segment: b'~',
            component: b':',
            repetition: Some(b'^'),
            release: None,
            decimal: b'.',
        }
    }

    /// The EDIFACT default delimiter set used when no `UNA` is present:
    /// component `:`, element `+`, decimal `.`, release `?`, segment `'`.
    pub fn edifact_default() -> Self {
        Delimiters {
            element: b'+',
            segment: b'\'',
            component: b':',
            repetition: None,
            release: Some(b'?'),
            decimal: b'.',
        }
    }
}

/// Detect which EDI family `bytes` belongs to from its leading magic prefix,
/// skipping any leading ASCII whitespace.
pub fn detect_family(bytes: &[u8]) -> Family {
    let start = leading_ws(bytes);
    let rest = &bytes[start..];
    if rest.starts_with(b"ISA") {
        Family::X12
    } else if rest.starts_with(b"UNA") || rest.starts_with(b"UNB") {
        Family::Edifact
    } else {
        Family::Unknown
    }
}

/// Count leading ASCII whitespace bytes (spaces, CR, LF, tab) so a stray BOM-less
/// blank line before the interchange does not defeat the magic-prefix match.
pub(crate) fn leading_ws(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .take_while(|b| matches!(b, b' ' | b'\r' | b'\n' | b'\t'))
        .count()
}

/// Sniff the four X12 delimiters out of the fixed-width ISA at the start of
/// `bytes`. Returns `None` if the input does not begin with a parseable ISA.
///
/// The algorithm (public syntax): the element separator is the byte right after
/// `ISA`; splitting the ISA on it recovers the 16 ISA elements; ISA11 is the
/// repetition separator (treated as "none" when it is the `U` version-4010
/// placeholder), ISA16 is the component separator, and the byte after ISA16 is
/// the segment terminator.
pub fn sniff_x12(bytes: &[u8]) -> Option<Delimiters> {
    let start = leading_ws(bytes);
    let rest = &bytes[start..];
    if !rest.starts_with(b"ISA") || rest.len() < 106 {
        return None;
    }
    // Byte index 3 (0-based) is the element separator.
    let element = rest[3];
    if element == 0 {
        return None;
    }
    // Walk the 16 element-separated fields of the fixed ISA to locate ISA11 and
    // ISA16 robustly (rather than assuming the canonical 106-byte field widths,
    // which a non-conforming sender may pad differently). The ISA is delimited by
    // `element`; the 17th separator ends ISA16 and the next byte is the segment
    // terminator.
    let mut sep_positions = Vec::with_capacity(17);
    for (i, &b) in rest.iter().enumerate() {
        if b == element {
            sep_positions.push(i);
            if sep_positions.len() == 16 {
                break;
            }
        }
    }
    if sep_positions.len() < 16 {
        return None;
    }
    // ISA11 is the field between separators 10 and 11 (0-based: the byte right
    // after the 11th separator is the start of ISA11... ). Elements are 1-based:
    // ISA01 follows separator[0]. ISA11 follows separator[10]; it is a single
    // char, then separator[11]. ISA16 follows separator[15]; it is a single
    // char, then the segment terminator.
    let isa11 = rest[sep_positions[10] + 1];
    let isa16_pos = sep_positions[15] + 1;
    let component = rest[isa16_pos];
    // The segment terminator is the byte immediately after ISA16.
    let seg_pos = isa16_pos + 1;
    if seg_pos >= rest.len() {
        return None;
    }
    let segment = rest[seg_pos];
    // ISA11 holds the repetition separator in 005010; in 004010 it is the literal
    // `U` standard-id placeholder, meaning "no repetition separator".
    let repetition = if isa11 == b'U' || isa11 == element || isa11 == segment {
        None
    } else {
        Some(isa11)
    };
    Some(Delimiters {
        element,
        segment,
        component,
        repetition,
        release: None,
        decimal: b'.',
    })
}

/// Parse the optional EDIFACT `UNA` service-string advice (9 fixed bytes:
/// `UNA` + component, element, decimal, release, reserved, segment). Returns
/// `None` if no `UNA` is present (the caller should then use
/// [`Delimiters::edifact_default`]).
pub fn sniff_edifact_una(bytes: &[u8]) -> Option<Delimiters> {
    let start = leading_ws(bytes);
    let rest = &bytes[start..];
    if !rest.starts_with(b"UNA") || rest.len() < 9 {
        return None;
    }
    let component = rest[3];
    let element = rest[4];
    let decimal = rest[5];
    let release = rest[6];
    // rest[7] is the reserved byte (a space).
    let segment = rest[8];
    Some(Delimiters {
        element,
        segment,
        component,
        repetition: None,
        release: Some(release),
        decimal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal but well-formed 005010 ISA using the canonical delimiter set,
    /// with `^` as a real repetition separator and `:` as the component sep.
    fn isa_5010() -> Vec<u8> {
        let isa = "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*:~";
        isa.as_bytes().to_vec()
    }

    #[test]
    fn detects_family() {
        assert_eq!(detect_family(b"ISA*00*"), Family::X12);
        assert_eq!(detect_family(b"UNB+UNOA"), Family::Edifact);
        assert_eq!(detect_family(b"UNA:+.? 'UNB"), Family::Edifact);
        assert_eq!(detect_family(b"\r\n  ISA*00"), Family::X12);
        assert_eq!(detect_family(b"GARBAGE"), Family::Unknown);
    }

    #[test]
    fn sniffs_canonical_5010() {
        let d = sniff_x12(&isa_5010()).expect("sniff");
        assert_eq!(d.element, b'*');
        assert_eq!(d.segment, b'~');
        assert_eq!(d.component, b':');
        assert_eq!(d.repetition, Some(b'^'));
    }

    #[test]
    fn sniffs_noncanonical_delimiters() {
        // element '|', component '>', segment '\n', 4010 'U' repetition placeholder.
        let isa = "ISA|00|          |00|          |ZZ|SENDER         |ZZ|RECEIVER       |240101|1200|U|00401|000000001|0|P|>\n";
        let d = sniff_x12(isa.as_bytes()).expect("sniff");
        assert_eq!(d.element, b'|');
        assert_eq!(d.component, b'>');
        assert_eq!(d.segment, b'\n');
        assert_eq!(d.repetition, None, "U placeholder => no repetition sep");
    }

    #[test]
    fn rejects_non_isa() {
        assert!(sniff_x12(b"NOPE not an interchange").is_none());
    }

    #[test]
    fn parses_una() {
        let d = sniff_edifact_una(b"UNA:+.? 'UNB+UNOA:1+").expect("una");
        assert_eq!(d.component, b':');
        assert_eq!(d.element, b'+');
        assert_eq!(d.release, Some(b'?'));
        assert_eq!(d.segment, b'\'');
    }
}
