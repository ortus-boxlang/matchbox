# MatchBox Benchmarks

- [Cross-runtime performance suite](runtime/README.md): verified AWFY workloads
  on MatchBox, Node.js and JVM BoxLang, with a standalone Python runner.
- [Compiler benchmarks](compiler/README.md): separate source-processing workloads.
- QoQ workloads below: query construction and execution comparisons.

## QoQ 1M Row Benchmark

This benchmark builds a 1,000,000 row in-memory query and then runs `AVG(value)` over it.
It is intended for comparing MatchBox and BoxLang on the same workload.

There are two script variants:
- `qoq_avg_1m.bxs` for MatchBox
- `qoq_avg_1m_boxlang.bxs` for BoxLang
- `qoq_many_small_queries.bxs` for MatchBox
- `qoq_many_small_queries_boxlang.bxs` for BoxLang

### Run MatchBox

```bash
/usr/bin/time -v cargo run --features qoq -- benchmarks/qoq_avg_1m.bxs
```

If you already have a local `matchbox` binary installed:

```bash
/usr/bin/time -v matchbox benchmarks/qoq_avg_1m.bxs
```

### Run BoxLang

```bash
/usr/bin/time -v boxlang benchmarks/qoq_avg_1m.bxs
```

If your BoxLang build requires the native queryNew signature, use the BoxLang-specific script:

```bash
/usr/bin/time -v boxlang benchmarks/qoq_avg_1m_boxlang.bxs
```

For a micro benchmark that stresses repeated QoQ execution instead of large-row aggregation:

```bash
/usr/bin/time -v cargo run --features qoq -- benchmarks/qoq_many_small_queries.bxs
/usr/bin/time -v boxlang benchmarks/qoq_many_small_queries_boxlang.bxs
```

### Notes

- `build ms` is the time to create the 1,000,000 row query.
- `qoq ms` is the time for the `AVG(value)` QoQ execution.
- `/usr/bin/time -v` gives peak RSS for the full process, which is the simplest memory comparison across runtimes.
- Run each command a few times and compare the steady-state numbers, not the first cold run only.
