"""Derive synthetic EDI variants from the committed complete fixture."""

from pathlib import Path
import argparse


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    base = (Path(__file__).parent / "fixtures/complete.edi").read_text()
    variants = {
        "complete.edi": base,
        "bad-count.edi": base.replace("SE*8*0001", "SE*99*0001"),
        "bad-quantity.edi": base.replace("PO1*2*5*EA", "PO1*2*unknown*EA"),
        "incomplete.edi": base[: base.index("GE*")],
        "no-se.edi": base[: base.index("SE*")],
        "no-transactions.edi": base[: base.index("ST*")] + "GE*0*1~\nIEA*1*000000400~\n",
    }
    args.destination.mkdir(parents=True, exist_ok=True)
    for name, text in variants.items():
        (args.destination / name).write_text(text)


if __name__ == "__main__":
    main()
