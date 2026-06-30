# vgi-x12

**Query raw ANSI ASC X12 EDI and UN/EDIFACT interchanges directly from DuckDB
SQL.** `vgi-x12` is a [VGI](https://query.farm) worker that DuckDB `ATTACH`es
over Apache Arrow IPC. It sniffs each interchange's own delimiters out of the
fixed-width ISA, explodes the ISA/GS/ST envelope into segment / element rows,
validates the structural segment counts and control numbers, detects the
transaction type, and — for the common healthcare / B2B transaction sets — emits
shaped, named-segment relational views.

It fills a real gap: there is **no DuckDB extension that parses X12 / EDIFACT at
all**, and the `hl7` worker is clinical HL7v2 messaging, not billing EDI.

- **Zero-infra, in-engine SQL** — no Java EDI stack, no Spark cluster; just
  `ATTACH` and `SELECT`.
- **No-egress local parse** — the worker makes **no outbound calls**. Parsing is
  100% local, which is load-bearing for PHI/PII (HIPAA) workloads, not a
  footnote. It composes with `vgi-mask` / `vgi-pii` for PHI redaction and the
  governance proxy for audit + row/column auth.
- **Public X12 syntax only** — see [Licensing & scope](#licensing--scope).

## Install & attach

```sql
INSTALL vgi FROM community;
LOAD vgi;

-- the worker is a local binary; no network/secret is needed for parsing
ATTACH 'x12' AS x12 (TYPE vgi, COMMAND 'x12-worker');
```

## SQL surface

Every table function takes a single overloaded `input` argument: a **file path
or glob** (the streaming hot path), an **inline interchange string**, or **inline
bytes**. The mode is auto-detected from the `ISA` / `UNA` / `UNB` magic prefix
(force it with `mode => 'path' | 'content'`). Every emitted row carries five
envelope keys (`interchange_ctrl` = ISA13, `group_ctrl` = GS06,
`transaction_ctrl` = ST02, `transaction_type` = ST01, `source_path`).

```sql
-- 1. Fully generic: one row per element, envelope keys carried down
SELECT interchange_ctrl, group_ctrl, transaction_ctrl, transaction_type,
       segment_index, segment_id, element_index, value
FROM x12.segments_elements('/data/claims/*.837')
WHERE segment_id = 'CLM';

-- 2. Envelope-only metadata (ISA/GS/ST) for routing / triage, with the
--    structural validity flags
SELECT interchange_ctrl, transaction_type, segment_count,
       se_count_ok, ge_count_ok, iea_count_ok
FROM x12.envelope('/data/inbound/*.edi');

-- 3. Shaped healthcare view: 835 remittance claim-payment rows
--    (public segment IDs only)
SELECT clp_claim_id, clp_status_code, clp_total_charge, clp_total_paid, clp_patient_resp
FROM x12.read_835('/data/era/*.835')
WHERE clp_total_paid < clp_total_charge;

-- 4. Sniff just the delimiters / type of inline content (scalars).
--    `read_text` is a DuckDB TABLE function, so it supplies the `content` column:
SELECT x12.delimiters(content), x12.transaction_type(content)
FROM read_text('/data/inbound/*.edi');

-- 5. EDIFACT interchange (UNA/UNB/UNH) explode
SELECT * FROM x12.edifact_segments('/data/orders/*.edi');

-- 6. Inline content (no file): the same functions accept a string directly
SELECT segment_id, element_index, value
FROM x12.segments_elements('ISA*00*          *00*          *ZZ*S              *ZZ*R              *240101*1200*^*00501*1*0*P*:~GS*HC*S*R*20240101*1200*1*X*005010~ST*837*0001~CLM*ACCT1*500~SE*3*0001~GE*1*1~IEA*1*1~');
```

## Function catalog

| function | kind | returns |
| --- | --- | --- |
| `segments(input)` | table | envelope keys + `segment_index`, `segment_id`, `elements LIST<VARCHAR>`, `byte_offset` |
| `segments_elements(input)` | table | envelope keys + `segment_index`, `segment_id`, `element_index`, `component_index`, `repetition_index`, `value` |
| `envelope(input)` | table | one row per ST: ISA/GS/ST metadata + `segment_count`, `se/ge/iea_count_ok`, `se/ge/iea_ctrl_match` |
| `read_835(input)` | table | remittance / ERA (parent `CLP`; `BPR`/`TRN`/`NM1` carried down; `CAS`/`SVC`/`REF`/`DTM` fan-out) |
| `read_837(input)` | table | claim (parent `CLM`; `NM1` pivoted by 85/IL/QC; `SBR` carried; `HI`/`SV1`/`SV2`/`DTP` fan-out) |
| `read_270(input)` / `read_271(input)` | table | eligibility inquiry / response (parent `HL`; `EB`/`DTP`/`AAA` fan-out) |
| `read_850(input)` | table | purchase order (parent `PO1`; `BEG` header; `N1`/`PER` parties) |
| `read_997(input)` / `read_999(input)` | table | functional acknowledgements (parent `AK2`; `AK3`/`AK4`/`AK5` or `IK3`/`IK4`/`IK5`) |
| `edifact_segments(input)` | table | UN/EDIFACT one row per element |
| `edifact_envelope(input)` | table | UN/EDIFACT one row per UNH message |
| `delimiters(content)` | scalar | `STRUCT(element, segment, component, repetition)` |
| `transaction_type(content)` | scalar | first ST01 (X12) or UNH02 message type (EDIFACT) |
| `x12_version()` | scalar | the worker's semver version string |

## How it works

X12 is a flat stream of **segments**, each a list of **elements**, each element
optionally a list of **components**. The grammar is entirely delimiter-driven and
the delimiters are discovered from the **fixed-width 106-byte ISA**: the element
separator is the byte right after `ISA`, ISA11 is the repetition separator (or
the 4010 `U` "none" placeholder), ISA16 is the component separator, and the byte
after ISA16 is the segment terminator. Non-canonical sets (e.g. element `|`,
component `>`, segment `\n`) are sniffed every time. EDIFACT uses the UNA service
string (with documented defaults) and a release/escape byte that is un-escaped
during the explode.

The envelope walk nests `ISA → GS → ST` (and `UNB → UNG → UNH`), captures the
control numbers, and computes **structural** validity flags by counting segments
and matching control numbers (`SE01`=count, `SE02`=ST02; `GE01`/`IEA01` likewise)
— public syntax that needs no copyrighted spec. Shaped views flatten the implicit
loops into **parent + child rows** keyed by shared envelope keys, and pivot `NM1`
loops by their entity-identifier qualifier.

Malformed or truncated input **never aborts the query**: parsing is total
(panic-free) and degrades to fewer rows / NULL validity flags.

## Building & testing

```bash
cargo build --release --bin x12-worker      # the worker binary
cargo test --workspace --all-features       # unit + golden-fixture tests
make test-sql                               # SQLLogic E2E over all 3 transports
make vgi-lint                               # metadata-quality gate (fail-on=info)
```

The SQLLogic suite (`test/sql/*.test`) runs against the signed community `vgi`
extension via `haybarn-unittest` across the subprocess / unix / HTTP transports.
See [`ci/README.md`](ci/README.md).

## Licensing & scope

Worker license: **MIT** (see [`LICENSE`](LICENSE)).

> **The real IP risk is the spec, not the code.** ASC X12 holds copyright on the
> transaction-set specifications, segment/element directories, code lists, and the
> HIPAA TR3 implementation guides, and prohibits their redistribution. This worker
> therefore ships only the **public X12 syntax**: envelope structure, delimiter
> rules, segment/element splitting, and segment IDs by their public names. It
> embeds **no** TR3 loop-hierarchy text or code-value descriptions. Shaped-column
> names use only the public segment ID + element position (`clp_total_paid` =
> `CLP04`), and raw codes are surfaced verbatim — human-readable code translation
> requires your own licensed X12 reference.

The parser is implemented in-house in the `x12-core` crate (MIT, serde-only, no
GPL dependencies). The GPL-3.0 `edi` crate is deliberately **not** used.

### v1 non-goals

No TR3 / SNIP content validation (loop membership, required elements, code-set
rules), no code-set translation, no acknowledgement *generation*, and no EDI
engine features (partner config, control-number sequencing, retransmission).
Structural (count + control-number + delimiter) validation only.

---

Part of the [Query.Farm](https://query.farm) VGI ecosystem. Copyright 2026 Query
Farm LLC.
