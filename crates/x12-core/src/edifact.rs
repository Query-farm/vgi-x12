//! UN/EDIFACT variant — the same delimiter-driven idea as X12 with different
//! envelopes (UNB/UNG/UNH … UNT/UNE/UNZ) and an explicit release/escape byte.
//! Kept in its own module so the two delimiter conventions never cross-
//! contaminate: sniffing branches on the magic prefix (`ISA` → X12, `UNA`/`UNB`
//! → EDIFACT). Public syntax only — no message-directory content.

use crate::delimiters::{self, Delimiters};
use crate::segment::{explode, Segment};

/// A whole EDIFACT interchange (UNB…UNZ) and its messages.
#[derive(Debug, Clone)]
pub struct EdiInterchange {
    pub delimiters: Delimiters,
    pub unb: Segment,
    pub unz: Option<Segment>,
    pub messages: Vec<EdiMessage>,
}

/// One EDIFACT message (UNH…UNT). `group_ref` is the enclosing UNG05 when the
/// message sits inside a functional group, else `None`. `segments` includes the
/// UNH and UNT framing (UNH first).
#[derive(Debug, Clone)]
pub struct EdiMessage {
    pub unh: Segment,
    pub unt: Option<Segment>,
    pub group_ref: Option<String>,
    pub segments: Vec<Segment>,
}

impl EdiInterchange {
    /// UNB05 — interchange control reference (the EDIFACT analogue of ISA13).
    pub fn control(&self) -> &str {
        self.unb.elem(5)
    }
}

impl EdiMessage {
    /// UNH01 — message reference number (the analogue of ST02).
    pub fn control(&self) -> &str {
        self.unh.elem(1)
    }
    /// UNH02-1 — message type (`ORDERS`, `INVOIC`, `DESADV`, …); the analogue of
    /// ST01 (`transaction_type`).
    pub fn message_type(&self, component: u8) -> &str {
        self.unh.elem_comp(2, 1, component)
    }
    /// The body segments strictly between UNH and UNT.
    pub fn body(&self) -> &[Segment] {
        if self.segments.is_empty() {
            return &[];
        }
        let start = 1.min(self.segments.len());
        let end = if self.unt.is_some() {
            self.segments.len() - 1
        } else {
            self.segments.len()
        };
        if start > end {
            return &[];
        }
        &self.segments[start..end]
    }
    /// Whether UNT01 equals the actual UNH..UNT segment count. `None` when no UNT.
    pub fn unt_count_ok(&self) -> Option<bool> {
        let unt = self.unt.as_ref()?;
        unt.elem(1)
            .trim()
            .parse::<usize>()
            .ok()
            .map(|n| n == self.segments.len())
    }
}

/// The delimiters governing an EDIFACT interchange: the `UNA` overrides the
/// defaults when present and takes precedence over UNB.
pub fn edifact_delimiters(bytes: &[u8]) -> Delimiters {
    delimiters::sniff_edifact_una(bytes).unwrap_or_else(Delimiters::edifact_default)
}

/// Parse `bytes` as one or more EDIFACT interchanges. Total and panic-free.
pub fn parse_edifact(bytes: &[u8]) -> Vec<EdiInterchange> {
    let mut out = Vec::new();
    if delimiters::detect_family(bytes) != delimiters::Family::Edifact {
        return out;
    }
    let delims = edifact_delimiters(bytes);
    let segs = explode(bytes, &delims);

    let mut cur_inter: Option<EdiInterchange> = None;
    let mut cur_group_ref: Option<String> = None;
    let mut cur_msg: Option<EdiMessage> = None;

    for seg in segs {
        match seg.id() {
            // The UNA service-string advice (if it survived as a "segment") is
            // metadata, not part of the interchange body — skip it.
            "UNA" => {}
            "UNB" => {
                flush_msg(&mut cur_msg, &mut cur_inter);
                flush_inter(&mut cur_inter, &mut out);
                cur_group_ref = None;
                cur_inter = Some(EdiInterchange {
                    delimiters: delims,
                    unb: seg,
                    unz: None,
                    messages: Vec::new(),
                });
            }
            "UNG" => {
                flush_msg(&mut cur_msg, &mut cur_inter);
                cur_group_ref = Some(seg.elem(5).to_string());
            }
            "UNE" => {
                flush_msg(&mut cur_msg, &mut cur_inter);
                cur_group_ref = None;
            }
            "UNH" => {
                flush_msg(&mut cur_msg, &mut cur_inter);
                cur_msg = Some(EdiMessage {
                    unh: seg.clone(),
                    unt: None,
                    group_ref: cur_group_ref.clone(),
                    segments: vec![seg],
                });
            }
            "UNT" => {
                if let Some(m) = cur_msg.as_mut() {
                    m.segments.push(seg.clone());
                    m.unt = Some(seg);
                }
                flush_msg(&mut cur_msg, &mut cur_inter);
            }
            "UNZ" => {
                flush_msg(&mut cur_msg, &mut cur_inter);
                if let Some(inter) = cur_inter.as_mut() {
                    inter.unz = Some(seg);
                }
                flush_inter(&mut cur_inter, &mut out);
                cur_group_ref = None;
            }
            _ => {
                if let Some(m) = cur_msg.as_mut() {
                    m.segments.push(seg);
                }
            }
        }
    }
    flush_msg(&mut cur_msg, &mut cur_inter);
    flush_inter(&mut cur_inter, &mut out);
    out
}

fn flush_msg(cur_msg: &mut Option<EdiMessage>, cur_inter: &mut Option<EdiInterchange>) {
    if let Some(m) = cur_msg.take() {
        if let Some(inter) = cur_inter.as_mut() {
            inter.messages.push(m);
        }
    }
}

fn flush_inter(cur_inter: &mut Option<EdiInterchange>, out: &mut Vec<EdiInterchange>) {
    if let Some(inter) = cur_inter.take() {
        out.push(inter);
    }
}

/// The first EDIFACT message type (UNH02-1) found in `bytes`, or empty.
pub fn first_message_type(bytes: &[u8]) -> String {
    let delims = edifact_delimiters(bytes);
    parse_edifact(bytes)
        .into_iter()
        .flat_map(|i| i.messages)
        .map(|m| m.message_type(delims.component).to_string())
        .find(|s| !s.is_empty())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orders() -> Vec<u8> {
        concat!(
            "UNA:+.? '",
            "UNB+UNOA:1+SENDER+RECEIVER+240101:1200+REF0001'",
            "UNG+ORDERS+SENDER+RECEIVER+240101:1200+1+UN+D:96A'",
            "UNH+MSG001+ORDERS:D:96A:UN'",
            "BGM+220+PO12345+9'",
            "DTM+137:20240101:102'",
            "UNT+4+MSG001'",
            "UNE+1+1'",
            "UNZ+1+REF0001'",
        )
        .as_bytes()
        .to_vec()
    }

    #[test]
    fn parses_orders() {
        let inters = parse_edifact(&orders());
        assert_eq!(inters.len(), 1);
        let i = &inters[0];
        assert_eq!(i.control(), "REF0001");
        assert_eq!(i.messages.len(), 1);
        let m = &i.messages[0];
        assert_eq!(m.control(), "MSG001");
        assert_eq!(m.message_type(i.delimiters.component), "ORDERS");
        assert_eq!(m.group_ref.as_deref(), Some("1"));
        assert_eq!(m.unt_count_ok(), Some(true));
        let body_ids: Vec<&str> = m.body().iter().map(|s| s.id()).collect();
        assert_eq!(body_ids, vec!["BGM", "DTM"]);
    }

    #[test]
    fn default_delimiters_without_una() {
        let bytes =
            b"UNB+UNOA:1+S+R+240101:1200+REF'UNH+M1+INVOIC:D:96A:UN'MOA+9:100'UNT+2+M1'UNZ+1+REF'";
        let inters = parse_edifact(bytes);
        assert_eq!(inters.len(), 1);
        assert_eq!(first_message_type(bytes), "INVOIC");
    }

    #[test]
    fn non_edifact_yields_empty() {
        assert!(parse_edifact(b"ISA*00* not edifact").is_empty());
    }
}
