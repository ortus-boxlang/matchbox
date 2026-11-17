'use strict';

// Load the unmodified upstream implementation; only timing/reporting is local.
const [modulePath, benchmarkName, warmupArg, iterationsArg] = process.argv.slice(2);
const warmup = Number(warmupArg);
const iterations = Number(iterationsArg);
if (!modulePath || !benchmarkName || !Number.isInteger(warmup) || warmup < 0
    || !Number.isInteger(iterations) || iterations < 1) {
  throw new Error('Expected module, benchmark name, warmup and measured iteration counts');
}
const bench = require(modulePath).newInstance();

function runBatch(instance, count) {
  let result = 0;
  for (let iteration = 0; iteration < count; iteration += 1) {
    result = instance.benchmark();
    if (!instance.verifyResult(result)) {
      throw new Error('Benchmark verification failed');
    }
  }
  return result;
}

runBatch(bench, warmup);
// Deliberately millisecond resolution, matching the portable BoxLang timer.
const started = Date.now();
const result = runBatch(bench, iterations);
const elapsed = Date.now() - started;
console.log(`BENCHMARK_RESULT ${JSON.stringify({
  benchmark: benchmarkName,
  warmup,
  iterations,
  result,
  elapsed_ms: elapsed,
})}`);
