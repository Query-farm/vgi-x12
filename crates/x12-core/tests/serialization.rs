use x12_core::serialization::{read_segments, SegmentRow, X12Writer};

const ERA: &[u8] = include_bytes!("../../../data/era_835.835");
const PIPE: &[u8] = include_bytes!("../../../data/pipe_delims.edi");

fn encode(rows: &[SegmentRow], repair: bool) -> Result<Vec<u8>, String> {
    let mut writer = X12Writer::new(repair);
    let mut bytes = Vec::new();
    for row in rows {
        bytes.extend(writer.push(
            row.segment.id(),
            row.segment.data_elements(),
            row.delimiters.element,
            row.delimiters.segment,
        )?);
    }
    writer.finish()?;
    Ok(bytes)
}

#[test]
fn round_trip_all_complete_x12_fixtures() {
    for input in [
        ERA,
        PIPE,
        include_bytes!("../../../data/ack_997.997"),
        include_bytes!("../../../data/claim_837.837"),
        include_bytes!("../../../data/po_850.850"),
        include_bytes!("../../../data/multi_group.edi"),
        include_bytes!("../../../data/eligibility_271.271"),
        include_bytes!("../../../data/inquiry_270.270"),
    ] {
        let rows = read_segments(input);
        let encoded = encode(&rows, false).unwrap();
        let reread = read_segments(&encoded);
        assert_eq!(rows.len(), reread.len());
        for (a, b) in rows.iter().zip(reread) {
            assert_eq!(a.segment.fields, b.segment.fields);
            assert_eq!(a.delimiters, b.delimiters);
            assert_eq!(a.interchange_ctrl, b.interchange_ctrl);
            assert_eq!(a.group_ctrl, b.group_ctrl);
            assert_eq!(a.transaction_ctrl, b.transaction_ctrl);
        }
        assert_eq!(rows.first().unwrap().segment.id(), "ISA");
        assert_eq!(rows.last().unwrap().segment.id(), "IEA");
    }
}

#[test]
fn mixed_delimiters_and_absolute_offsets() {
    let bytes = [b"\r\n\t".as_slice(), ERA, PIPE, ERA].concat();
    let rows = read_segments(&bytes);
    assert_eq!(
        rows.len(),
        read_segments(ERA).len() * 2 + read_segments(PIPE).len()
    );
    assert_eq!(rows[0].segment.byte_offset, 3);
    for row in &rows {
        assert!(bytes[row.segment.byte_offset as usize..].starts_with(row.segment.id().as_bytes()));
    }
    let encoded = encode(&rows, false).unwrap();
    let isa_seps: Vec<_> = read_segments(&encoded)
        .into_iter()
        .filter(|r| r.segment.id() == "ISA")
        .map(|r| r.delimiters.element)
        .collect();
    assert_eq!(isa_seps, b"*|*");
}

#[test]
fn preserve_empty_elements_composites_repetitions_and_padding() {
    let input = String::from_utf8(ERA.to_vec())
        .unwrap()
        .replace("BPR*I*1500*C*ACH", "BPR**A:B^C**");
    let encoded =
        String::from_utf8(encode(&read_segments(input.as_bytes()), false).unwrap()).unwrap();
    assert!(encoded.contains("BPR**A:B^C**~"));
    assert!(encoded.contains("*SENDER         *"));
}

#[test]
fn keys_end_at_each_scope_and_stray_segments_are_retained() {
    let input = String::from_utf8(ERA.to_vec())
        .unwrap()
        .replace("GS*HP", "ZZZ*stray~GS*HP");
    let rows = read_segments(input.as_bytes());
    let find = |id| rows.iter().find(|r| r.segment.id() == id).unwrap();
    assert_eq!(find("ZZZ").segment.elem(1), "stray");
    assert_eq!(find("SE").transaction_ctrl, "0001");
    assert!(find("GE").transaction_ctrl.is_empty());
    assert!(find("IEA").group_ctrl.is_empty());
    assert!(encode(&rows, false).unwrap_err().contains("outside ST/SE"));
}

#[test]
fn edited_counts_require_explicit_recalculation() {
    let mut rows = read_segments(ERA);
    rows.retain(|r| r.segment.id() != "BPR");
    assert!(encode(&rows, false).unwrap_err().contains("SE01 count"));
    for row in &mut rows {
        if matches!(row.segment.id(), "GE" | "IEA") {
            row.segment.fields[1] = "999".into();
        }
    }
    let encoded = encode(&rows, true).unwrap();
    let interchanges = x12_core::envelope::parse_x12(&encoded);
    assert_eq!(interchanges[0].iea_count_ok(), Some(true));
    assert_eq!(interchanges[0].groups[0].ge_count_ok(), Some(true));
    assert_eq!(
        interchanges[0].groups[0].transactions[0].se_count_ok(),
        Some(true)
    );
    let reread = read_segments(&encoded);
    for (a, b) in rows.iter().zip(reread) {
        if matches!(a.segment.id(), "ISA" | "GS" | "ST") {
            assert_eq!(a.segment.fields, b.segment.fields);
        }
    }
}

#[test]
fn refuse_incomplete_or_mismatched_envelopes_even_with_repair() {
    let rows = read_segments(ERA);
    for id in ["ISA", "GS", "ST", "SE", "GE", "IEA"] {
        let edited: Vec<_> = rows
            .iter()
            .filter(|r| r.segment.id() != id)
            .cloned()
            .collect();
        assert!(encode(&edited, true).is_err(), "missing {id}");
    }
    for id in ["SE", "GE", "IEA"] {
        let mut edited = rows.clone();
        edited
            .iter_mut()
            .find(|r| r.segment.id() == id)
            .unwrap()
            .segment
            .fields[2] = "other".into();
        assert!(encode(&edited, true)
            .unwrap_err()
            .contains("control number"));
    }
    let mut swapped = rows.clone();
    swapped.swap(2, 3);
    assert!(encode(&swapped, true).is_err());
}

#[test]
fn reject_invalid_isa_delimiters_and_data() {
    for value in [
        "embedded*separator",
        "embedded~terminator",
        "non-ascii-é",
        "line\nbreak",
    ] {
        let mut rows = read_segments(ERA);
        rows[3].segment.fields[1] = value.into();
        assert!(encode(&rows, false).is_err());
    }
    let mut rows = read_segments(ERA);
    rows[0].segment.fields[6] = "short".into();
    assert!(encode(&rows, false).unwrap_err().contains("fixed-width"));
    let mut rows = read_segments(ERA);
    rows[0].segment.fields[16] = "^".into();
    assert!(encode(&rows, false).unwrap_err().contains("conflicting"));
    let mut rows = read_segments(ERA);
    rows[3].delimiters.element = b'|';
    assert!(encode(&rows, false)
        .unwrap_err()
        .contains("delimiters changed"));
}

#[test]
fn empty_and_malformed_input_are_total() {
    assert_eq!(encode(&[], false).unwrap(), b"");
    for input in [
        b"".as_slice(),
        b"garbage",
        b"ISA",
        b"UNA:+.? '",
        &[255; 200],
    ] {
        assert!(read_segments(input).is_empty());
    }
    assert!(encode(
        &read_segments(include_bytes!("../../../data/truncated.edi")),
        true
    )
    .is_err());
}
