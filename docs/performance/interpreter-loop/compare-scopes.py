#!/usr/bin/env python3
"""Repartition saved traces only; never launch, build, or measure a runtime."""
import argparse
from collections import Counter
import json
from pathlib import Path
import re


SPANS = [
    ("GET_LOCAL read", "GET_LOCAL", "0x418779", "0x41be77", 182),
    ("FOR_LOOP_STEP read", "FOR_LOOP_STEP", "0x40e958", "0x411412", 185),
    ("FOR_LOOP_STEP write", "FOR_LOOP_STEP", "0x41cf69", "0x41dd21", 186),
]
# Native anchors delimit executed sequences, NOT all reachable PCs in an interval.
FRAGMENTS = [
    ("first read frame/mode prefix", "GET_LOCAL", "0x418779", "0x4187c2", 23),
    ("first read existing stack fallback", "GET_LOCAL", "0x41bdf2", "0x41be77", 43),
    ("write existing stack fallback", "FOR_LOOP_STEP", "0x41dc82", "0x41dd21", 48),
    ("read indexed-load tail", "GET_LOCAL", "0x41be57", "0x41be77", 11),
    ("write indexed-store tail", "FOR_LOOP_STEP", "0x41dcfa", "0x41dd21", 12),
]
HEADER = (0x40c2df, 0x40ca1d)
FIBER = "<alloc::vec::Vec<matchbox_vm::vm::BxFiber> as core::ops::index::Index<usize>>::index"
VM = "<matchbox_vm::vm::VM>::run_fiber_inner"


def symbol(row):
    return re.split(r" \+ | in section ", row["symbol"])[0]


def cycle(report, iteration):
    rows = []
    for row in report["trace"]:
        if row["iteration"] != iteration:
            continue
        if rows and row["pc"] == rows[-1]["pc"] and row["asm"].startswith("rep"):
            continue
        rows.append(row)
    return rows


def interval(rows, context, start, end):
    def locate(pc):
        indices = [i for i, row in enumerate(rows)
                   if row["handler_context"] == context and row["pc"] == pc]
        assert len(indices) == 1, (context, pc, indices)
        return indices[0]
    lo, hi = locate(start), locate(end) + 1
    assert lo < hi
    return set(range(lo, hi))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", nargs="?", type=Path, default=(
        Path(__file__).resolve().parents[3] / "benchmarks/instructions/results"
        / "native-ledger-20261030T201844Z"))
    args = parser.parse_args()
    output = {"kind": "static reanalysis of existing traces, NOT new measurement", "cycles": []}
    for run in ("mb-early", "mb-late"):
        report = json.loads((args.evidence / run / "report.json").read_text())
        assert report["status"] == "complete" and report["exit_code"] == 0
        assert (args.evidence / f"{run}.stdout").read_text().count("500001500000") == 1
        for iteration in (0, 1):
            rows = cycle(report, iteration)
            assert len(rows) == 2173
            selected = set()
            spans = []
            for name, context, start, end, expected in SPANS:
                indices = interval(rows, context, start, end)
                assert not selected & indices
                assert len(indices) == expected
                assert all(not HEADER[0] <= int(rows[i]["pc"], 16) < HEADER[1]
                           for i in indices)
                selected |= indices
                spans.append({"name": name, "instructions": len(indices),
                              "self_by_function": dict(Counter(symbol(rows[i]) for i in sorted(indices)))})
            fragments = {}
            for name, context, start, end, expected in FRAGMENTS:
                indices = interval(rows, context, start, end)
                assert indices <= selected and len(indices) == expected
                fragments[name] = len(indices)
            fiber_calls = [i for i in range(len(rows) - 1)
                           if rows[i]["asm"].startswith("call") and symbol(rows[i + 1]) == FIBER]
            assert len(fiber_calls) == 49
            overlap = sum(i in selected for i in fiber_calls)
            assert overlap == 14
            fiber_self = sum(symbol(rows[i]) == FIBER for i in selected)
            assert fiber_self == 84
            vm_self = sum(symbol(rows[i]) == VM for i in selected)
            assert vm_self == 216
            output["cycles"].append({
                "run": run, "iteration": iteration, "spans": spans,
                "selected": len(selected), "remaining": len(rows) - len(selected),
                "selected_vm_self": vm_self, "selected_fiber_self": fiber_self,
                "fiber_calls": len(fiber_calls), "selected_fiber_calls": overlap,
                "fiber_call_plus_self_overlap": overlap + fiber_self,
                "existing_replacement_fragments": fragments,
            })
    # Prediction inputs are explicitly estimates, separate from observed spans.
    gross = 553
    repeated_lookup_estimate = 3 * 23 + 2 * 43 + 48
    # Reuse the first frame/fiber acquisition; retain indexed loads/stores.
    # The write tail also needs one stack_base load, outside its 12-row suffix.
    reused_reference_estimate = 3 * 23 + 2 * 11 + 12 + 1
    contingency = 40
    output["P1_model"] = {
        "existing_span": gross,
        "estimated_replacement_reusing_reference": reused_reference_estimate,
        "estimated_replacement_repeating_existing_fallback": repeated_lookup_estimate,
        "assumed_codegen_contingency_NOT_measured_bound": [0, contingency],
        "conditional_net_reduction": [gross - repeated_lookup_estimate - contingency, gross - reused_reference_estimate],
        "not_a_compiled_replacement_or_hardware_result": True,
    }
    print(json.dumps(output, indent=2))


if __name__ == "__main__":
    main()
