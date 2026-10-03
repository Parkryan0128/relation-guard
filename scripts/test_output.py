"""Keep generated test results outside tracked source files."""
import os
from pathlib import Path


def output_path(variable, filename):
    root = Path(__file__).resolve().parents[1]
    path = Path(os.environ.get(variable, str(root / 'target/test-results' / filename)))
    path.parent.mkdir(parents=True, exist_ok=True)
    return path
