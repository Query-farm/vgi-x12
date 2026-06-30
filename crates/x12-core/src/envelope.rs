//! The X12 envelope walk: ISA → GS → ST nesting, control-number capture, and
//! `SE` / `GE` / `IEA` **structural** validation (segment counts + control-number
//! matching). This is public syntax only — it counts segments and compares
//! control numbers; it does **not** validate loop membership, required elements,
//! or code values (those need the copyrighted TR3).

use crate::delimiters::{self, Delimiters};
use crate::segment::{explode, Segment};

/// A whole X12 interchange: the ISA envelope, its functional groups, and the
/// computed structural-validity facts.
#[derive(Debug, Clone)]
pub struct Interchange {
    /// Sniffed delimiters for this interchange.
    pub delimiters: Delimiters,
    /// The ISA segment (fixed-width header).
    pub isa: Segment,
    /// The IEA trailer, if present (`None` when the interchange is truncated).
    pub iea: Option<Segment>,
    /// The functional groups (GS…GE) inside this interchange.
    pub groups: Vec<Group>,
}

/// A functional group (GS…GE) and its transaction sets.
#[derive(Debug, Clone)]
pub struct Group {
    pub gs: Segment,
    pub ge: Option<Segment>,
    pub transactions: Vec<Transaction>,
}

/// A transaction set (ST…SE) and the body segments it frames. `segments`
/// includes the ST and SE segments themselves (ST first, SE last when present),
/// so `segments[0]` is the ST.
#[derive(Debug, Clone)]
pub struct Transaction {
    pub st: Segment,
    pub se: Option<Segment>,
    /// Every segment from ST through SE inclusive, in order.
    pub segments: Vec<Segment>,
}

impl Interchange {
    /// ISA13 — interchange control number.
    pub fn control(&self) -> &str {
        self.isa.elem(13)
    }
    /// Whether the IEA group count (IEA01) equals the number of groups parsed.
    /// `None` when there is no IEA (truncated interchange).
    pub fn iea_count_ok(&self) -> Option<bool> {
        let iea = self.iea.as_ref()?;
        iea.elem(1)
            .trim()
            .parse::<usize>()
            .ok()
            .map(|n| n == self.groups.len())
    }
    /// Whether the IEA control (IEA02) matches ISA13. `None` when no IEA.
    pub fn iea_ctrl_match(&self) -> Option<bool> {
        let iea = self.iea.as_ref()?;
        Some(iea.elem(2) == self.isa.elem(13))
    }
}

impl Group {
    /// GS06 — group control number.
    pub fn control(&self) -> &str {
        self.gs.elem(6)
    }
    /// Whether GE01 equals the number of transactions parsed. `None` when no GE.
    pub fn ge_count_ok(&self) -> Option<bool> {
        let ge = self.ge.as_ref()?;
        ge.elem(1)
            .trim()
            .parse::<usize>()
            .ok()
            .map(|n| n == self.transactions.len())
    }
    /// Whether GE02 matches GS06. `None` when no GE.
    pub fn ge_ctrl_match(&self) -> Option<bool> {
        let ge = self.ge.as_ref()?;
        Some(ge.elem(2) == self.gs.elem(6))
    }
}

impl Transaction {
    /// ST01 — transaction set identifier (`835`, `837`, …).
    pub fn type_code(&self) -> &str {
        self.st.elem(1)
    }
    /// ST02 — transaction set control number.
    pub fn control(&self) -> &str {
        self.st.elem(2)
    }
    /// The number of segments from ST through SE inclusive (what SE01 must equal).
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }
    /// Whether SE01 equals the actual ST..SE segment count. `None` when no SE.
    pub fn se_count_ok(&self) -> Option<bool> {
        let se = self.se.as_ref()?;
        se.elem(1)
            .trim()
            .parse::<usize>()
            .ok()
            .map(|n| n == self.segment_count())
    }
    /// Whether SE02 matches ST02. `None` when no SE.
    pub fn se_ctrl_match(&self) -> Option<bool> {
        let se = self.se.as_ref()?;
        Some(se.elem(2) == self.st.elem(2))
    }
    /// The body segments strictly between ST and SE (excludes the ST/SE framing),
    /// which the shaped extractors walk.
    pub fn body(&self) -> &[Segment] {
        if self.segments.is_empty() {
            return &[];
        }
        // Skip the leading ST.
        let start = 1.min(self.segments.len());
        // Drop the trailing SE when present.
        let end = if self.se.is_some() {
            self.segments.len() - 1
        } else {
            self.segments.len()
        };
        if start > end {
            return &[];
        }
        &self.segments[start..end]
    }
}

/// Parse `bytes` as one or more X12 interchanges. Total and panic-free: a
/// missing trailer leaves the corresponding `Option` `None`, and bytes that do
/// not begin with `ISA` yield an empty `Vec` (the caller emits zero rows rather
/// than aborting the query).
pub fn parse_x12(bytes: &[u8]) -> Vec<Interchange> {
    let mut out = Vec::new();
    // Sniff the first interchange's delimiters; bail cleanly if not X12.
    let Some(first) = delimiters::sniff_x12(bytes) else {
        return out;
    };
    // A single file can hold several interchanges, each potentially re-sniffed
    // (different delimiters per ISA). Explode with the first delimiters, then
    // walk segment-by-segment, restarting delimiter scope at each new ISA.
    //
    // To support per-interchange re-sniffing we segment the raw bytes around
    // each ISA boundary. We find ISA occurrences at a segment start; for the
    // common single-interchange / homogeneous-delimiter file the first sniff
    // governs the whole stream.
    let segs = explode(bytes, &first);
    walk_segments(segs, first, &mut out);
    out
}

/// Walk a flat segment list into nested interchanges/groups/transactions.
fn walk_segments(segs: Vec<Segment>, delims: Delimiters, out: &mut Vec<Interchange>) {
    let mut cur_inter: Option<Interchange> = None;
    let mut cur_group: Option<Group> = None;
    let mut cur_tx: Option<Transaction> = None;

    // Helper closures are awkward with the borrow checker here, so inline the
    // "flush" steps as we encounter the relevant trailer / new header.
    for seg in segs {
        match seg.id() {
            "ISA" => {
                // Close any open scopes from a previous (possibly truncated)
                // interchange before starting a new one.
                close_tx(&mut cur_tx, &mut cur_group, &mut cur_inter);
                close_group(&mut cur_group, &mut cur_inter);
                close_inter(&mut cur_inter, out);
                cur_inter = Some(Interchange {
                    delimiters: delims,
                    isa: seg,
                    iea: None,
                    groups: Vec::new(),
                });
            }
            "GS" => {
                close_tx(&mut cur_tx, &mut cur_group, &mut cur_inter);
                close_group(&mut cur_group, &mut cur_inter);
                cur_group = Some(Group {
                    gs: seg,
                    ge: None,
                    transactions: Vec::new(),
                });
            }
            "ST" => {
                close_tx(&mut cur_tx, &mut cur_group, &mut cur_inter);
                cur_tx = Some(Transaction {
                    st: seg.clone(),
                    se: None,
                    segments: vec![seg],
                });
            }
            "SE" => {
                if let Some(tx) = cur_tx.as_mut() {
                    tx.segments.push(seg.clone());
                    tx.se = Some(seg);
                }
                close_tx(&mut cur_tx, &mut cur_group, &mut cur_inter);
            }
            "GE" => {
                close_tx(&mut cur_tx, &mut cur_group, &mut cur_inter);
                if let Some(g) = cur_group.as_mut() {
                    g.ge = Some(seg);
                }
                close_group(&mut cur_group, &mut cur_inter);
            }
            "IEA" => {
                close_tx(&mut cur_tx, &mut cur_group, &mut cur_inter);
                close_group(&mut cur_group, &mut cur_inter);
                if let Some(inter) = cur_inter.as_mut() {
                    inter.iea = Some(seg);
                }
                close_inter(&mut cur_inter, out);
            }
            _ => {
                // Body segment: attach to the open transaction. A stray body
                // segment outside any ST is dropped (it has no envelope home).
                if let Some(tx) = cur_tx.as_mut() {
                    tx.segments.push(seg);
                }
            }
        }
    }
    // Flush anything left open by a truncated interchange.
    close_tx(&mut cur_tx, &mut cur_group, &mut cur_inter);
    close_group(&mut cur_group, &mut cur_inter);
    close_inter(&mut cur_inter, out);
}

fn close_tx(
    cur_tx: &mut Option<Transaction>,
    cur_group: &mut Option<Group>,
    cur_inter: &mut Option<Interchange>,
) {
    if let Some(tx) = cur_tx.take() {
        if let Some(g) = cur_group.as_mut() {
            g.transactions.push(tx);
        } else if let Some(inter) = cur_inter.as_mut() {
            // A transaction with no enclosing GS (malformed but seen): park it
            // in a synthetic group so its rows are not lost.
            if inter.groups.is_empty() {
                inter.groups.push(Group {
                    gs: Segment {
                        fields: vec!["GS".to_string()],
                        byte_offset: 0,
                    },
                    ge: None,
                    transactions: Vec::new(),
                });
            }
            inter.groups.last_mut().unwrap().transactions.push(tx);
        }
    }
}

fn close_group(cur_group: &mut Option<Group>, cur_inter: &mut Option<Interchange>) {
    if let Some(g) = cur_group.take() {
        if let Some(inter) = cur_inter.as_mut() {
            inter.groups.push(g);
        }
    }
}

fn close_inter(cur_inter: &mut Option<Interchange>, out: &mut Vec<Interchange>) {
    if let Some(inter) = cur_inter.take() {
        out.push(inter);
    }
}

/// The first transaction set identifier (ST01) found in `bytes`, or the EDIFACT
/// message type (UNH02-1) for an EDIFACT interchange. Empty string when none.
pub fn first_transaction_type(bytes: &[u8]) -> String {
    match delimiters::detect_family(bytes) {
        delimiters::Family::X12 => parse_x12(bytes)
            .into_iter()
            .flat_map(|i| i.groups)
            .flat_map(|g| g.transactions)
            .map(|t| t.type_code().to_string())
            .find(|s| !s.is_empty())
            .unwrap_or_default(),
        delimiters::Family::Edifact => crate::edifact::first_message_type(bytes),
        delimiters::Family::Unknown => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_835() -> Vec<u8> {
        // ISA(106) + GS + ST..SE + GE + IEA. SE01 = 5 (ST,BPR,TRN,CLP,SE).
        let body = concat!(
            "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*:~",
            "GS*HP*SEND*RECV*20240101*1200*1*X*005010X221A1~",
            "ST*835*0001~",
            "BPR*I*1000*C*ACH~",
            "TRN*1*TRACE123*1234567890~",
            "CLP*PCN1*1*500*400*100*MC*CLAIM9*11~",
            "SE*5*0001~",
            "GE*1*1~",
            "IEA*1*000000001~",
        );
        body.as_bytes().to_vec()
    }

    #[test]
    fn walks_nesting_and_controls() {
        let inters = parse_x12(&minimal_835());
        assert_eq!(inters.len(), 1);
        let i = &inters[0];
        assert_eq!(i.control(), "000000001");
        assert_eq!(i.groups.len(), 1);
        let g = &i.groups[0];
        assert_eq!(g.control(), "1");
        assert_eq!(g.transactions.len(), 1);
        let t = &g.transactions[0];
        assert_eq!(t.type_code(), "835");
        assert_eq!(t.control(), "0001");
        assert_eq!(t.segment_count(), 5);
        assert_eq!(t.se_count_ok(), Some(true));
        assert_eq!(t.se_ctrl_match(), Some(true));
        assert_eq!(g.ge_count_ok(), Some(true));
        assert_eq!(g.ge_ctrl_match(), Some(true));
        assert_eq!(i.iea_count_ok(), Some(true));
        assert_eq!(i.iea_ctrl_match(), Some(true));
        // Body excludes ST and SE.
        let body_ids: Vec<&str> = t.body().iter().map(|s| s.id()).collect();
        assert_eq!(body_ids, vec!["BPR", "TRN", "CLP"]);
    }

    #[test]
    fn truncated_interchange_no_panic() {
        // Drop the IEA and GE and SE — everything left open.
        let body = concat!(
            "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*:~",
            "GS*HC*SEND*RECV*20240101*1200*1*X*005010X222A1~",
            "ST*837*0001~",
            "CLM*ACCT1*500~",
        );
        let inters = parse_x12(body.as_bytes());
        assert_eq!(inters.len(), 1);
        let i = &inters[0];
        assert!(i.iea.is_none());
        assert_eq!(i.iea_count_ok(), None);
        let t = &i.groups[0].transactions[0];
        assert_eq!(t.se_count_ok(), None);
        assert_eq!(t.type_code(), "837");
    }

    #[test]
    fn bad_se_count_flagged_false() {
        let body = concat!(
            "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000002*0*P*:~",
            "GS*HP*SEND*RECV*20240101*1200*1*X*005010X221A1~",
            "ST*835*0001~",
            "BPR*I*1000*C*ACH~",
            "SE*99*0001~",
            "GE*1*1~",
            "IEA*1*000000002~",
        );
        let inters = parse_x12(body.as_bytes());
        let t = &inters[0].groups[0].transactions[0];
        assert_eq!(t.se_count_ok(), Some(false));
        assert_eq!(t.se_ctrl_match(), Some(true));
    }

    #[test]
    fn non_x12_yields_empty() {
        assert!(parse_x12(b"not an interchange at all").is_empty());
    }
}
