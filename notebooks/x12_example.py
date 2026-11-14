"""Presentation for authored X12 input audits and EDIFACT field boundaries."""

import hashlib
import json
from pathlib import Path

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
from matplotlib.colors import ListedColormap
from vgi_notebooks import finish

SOURCE = "Source: authored synthetic EDI alternatives · VGI X12 v0.5.0 · no real business or health records"


def verify_fixtures():
    """Require exactly the recorded immutable input bytes."""
    manifest = json.loads(Path("fixtures/provenance.json").read_text())
    for name, expected in manifest["files"].items():
        if hashlib.sha256((Path("fixtures") / name).read_bytes()).hexdigest() != expected["sha256"]:
            raise ValueError(f"Fixture differs from its recorded provenance: {name}")


def validate_switch(value, name):
    """Validate a visible Boolean analytical choice."""
    if type(value) is not bool:
        raise ValueError(f"{name} must be True or False.")


def envelope_evidence(audit):
    """Retain unavailable structural checks separately from failures."""
    columns = [
        "se_count_ok",
        "ge_count_ok",
        "iea_count_ok",
        "se_ctrl_match",
        "ge_ctrl_match",
        "iea_ctrl_match",
    ]
    values = np.array(
        [
            [0 if pd.isna(v) else 2 if bool(v) else 1 for v in row]
            for row in audit[columns].itertuples(index=False, name=None)
        ]
    )
    fig, ax = plt.subplots(figsize=(12, 7))
    ax.imshow(
        values,
        cmap=ListedColormap(["#e2e6e3", "#f1d4ba", "#b9ddcf"]),
        vmin=0,
        vmax=2,
        aspect="auto",
    )
    ax.set_xticks(
        range(6), ["SE count", "GE count", "IEA count", "SE control", "GE control", "IEA control"]
    )
    ax.set_yticks(range(len(audit)), audit.document_id)
    ax.set_xticks(np.arange(-0.5, 6, 1), minor=True)
    ax.set_yticks(np.arange(-0.5, len(audit), 1), minor=True)
    ax.grid(False)
    ax.grid(which="minor", color="white", linewidth=3)
    ax.tick_params(which="minor", bottom=False, left=False)
    for y, row in enumerate(values):
        for x, v in enumerate(row):
            ax.text(
                x,
                y,
                ["—", "Fail", "Pass"][v],
                ha="center",
                va="center",
                fontsize=11,
                color="#253d31",
            )
    finish(
        fig,
        "Unknown checks are different from failures",
        "Every inventoried input stays visible · — = no available check result",
        SOURCE,
        name="01-envelope-evidence",
    )


def subtotals(summary, allow_incomplete):
    """Keep selected amounts and selected/extracted line denominators together."""
    fig, ax = plt.subplots(figsize=(12, 7))
    ax.barh(
        range(len(summary)),
        summary.selected_subtotal.astype(float),
        color=["#267358" if v > 0 else "#8b9790" for v in summary.selected_subtotal],
        height=0.55,
    )
    for i, r in enumerate(summary.itertuples()):
        ax.text(
            float(r.selected_subtotal) + 2,
            i,
            f"{r.selected_subtotal:.2f} · {r.selected_lines}/{r.extracted_lines} lines",
            va="center",
            fontsize=11,
        )
    ax.set_yticks(range(len(summary)), summary.document_id.tolist())
    ax.invert_yaxis()
    ax.set_xlim(0, 145)
    ax.set_xlabel("Selected numeric line subtotal · fictional fixture price units")
    ax.grid(axis="y", visible=False)
    finish(
        fig,
        "Selection does not repair the interchange",
        f"Incomplete-envelope preview {'enabled' if allow_incomplete else 'disabled'} · zero means no selected amount",
        SOURCE,
        name="02-selected-subtotals",
    )


def edifact_counts(data, escape_plus):
    """Count element/component rows without relabeling them as segments."""
    fig, ax = plt.subplots(figsize=(11.5, 5.8))
    bars = ax.barh(
        data.segment_id,
        data.decoded_value_rows,
        color=["#c57b40" if x == "FTX" else "#267358" for x in data.segment_id],
        height=0.48,
    )
    ax.bar_label(bars, padding=6, fmt="%d", weight="bold")
    ax.invert_yaxis()
    ax.set_xlim(0, 6)
    ax.set_xticks(range(7))
    ax.set_xlabel("Decoded element/component rows per segment · count")
    ax.grid(axis="y", visible=False)
    finish(
        fig,
        "A field boundary can change while segment counts agree",
        f"Release character {'present' if escape_plus else 'removed'} · 3 message segments · {int(data.decoded_value_rows.sum())} decoded value rows",
        SOURCE,
        name="03-edifact-value-counts",
    )
