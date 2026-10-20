import csv
import json
import sys
from pathlib import Path


def write_records(records: list[dict], path: str | None) -> None:
    """Writes JSON, JSONL or CSV based on the file extension, or JSON to stdout."""
    if not path or path == "-":
        json.dump(records, sys.stdout, indent=2, ensure_ascii=False)
        sys.stdout.write("\n")
        return
    suffix = Path(path).suffix.lower()
    with open(path, "w", encoding="utf-8", newline="") as f:
        if suffix == ".csv":
            keys = list(dict.fromkeys(k for r in records for k in r))
            writer = csv.DictWriter(f, fieldnames=keys)
            writer.writeheader()
            writer.writerows(records)
        elif suffix == ".jsonl":
            for r in records:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")
        else:
            json.dump(records, f, indent=2, ensure_ascii=False)
