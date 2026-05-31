"""Skill regression and lift measurement suite (040).

Benchmark judge import: sys.path shim to benchmark/021 is applied here.
After importing this package, `from runner.judge import score_result` works.
"""
import sys
import os

_BENCHMARK_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "benchmark", "021")
)
if _BENCHMARK_DIR not in sys.path:
    sys.path.insert(0, _BENCHMARK_DIR)
