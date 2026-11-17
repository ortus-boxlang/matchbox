#!/usr/bin/env python3
"""Frozen one-shot evaluator and checks. NOT an experiment-loop controller."""
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import statistics
import subprocess
import sys
import time
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
AUTO = ROOT / '.auto'
ART = AUTO / 'artifacts'
BINARY = ROOT / 'target/build/release/examples/instruction_interpreter'
ALLOW = ['crates/matchbox-vm/src/vm/' + f for f in ('mod.rs','calls.rs','scopes.rs','struct_scope.rs')]
PREPARATION = False
VM_FEATURES = 'jit,bif-io,bif-http,bif-zip,bif-crypto,bif-cli,bif-jni,bif-datasource'
SUITES = {
 'vm_lib': ['-p','matchbox_vm','--features',VM_FEATURES,'--lib'],
 'vm_canonical_keys': ['-p','matchbox_vm','--features',VM_FEATURES,'--test','canonical_keys'],
 'compiler_error_reporting': ['-p','matchbox_compiler','--test','error_reporting'],
 'compiler_member_shaking': ['-p','matchbox_compiler','--test','member_shaking'],
 'compiler_cst_tooling': ['-p','matchbox_compiler','--test','cst_tooling'],
 'integration': ['-p','matchbox','--test','integration_tests'],
 'compatibility': ['-p','matchbox','--test','boxlang_compat_tests'],
}
PROBES = ['functions.bxs','vm_functions.bxs','vm_function_execution.bxs',
          'vm_scope_bindings.bxs','vm_lexical_closures.bxs','vm_closure_loop_variable.bxs',
          'vm_required_arguments.bxs','vm_typed_functions.bxs','guarded_frame_wide_arguments.bxs']


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def stamp():
    return datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ')


def write(path, data):
    path.write_text(json.dumps(data, indent=2) + '\n')


def limit_memory():
    resource.setrlimit(resource.RLIMIT_AS, (8 * 1024**3, 8 * 1024**3))


def command(args, name, seconds=600, permit_test_failure=False):
    start_path = ART/'session-start.json'
    if start_path.exists():
        remaining = 3600 - (time.time() - json.loads(start_path.read_text())['start'])
        if remaining <= 0:
            raise RuntimeError('60-minute campaign budget exhausted')
        seconds = min(seconds, remaining)
    result = subprocess.run(['timeout','--kill-after=10s',f'{seconds}s',*map(str,args)],
                            cwd=ROOT, capture_output=True, text=True,
                            preexec_fn=limit_memory)
    text = result.stdout + result.stderr
    (ART / f'{name}.log').write_text(text)
    if result.returncode and not (permit_test_failure and result.returncode == 101):
        raise RuntimeError(f'{name} exit={result.returncode}; see .auto/artifacts/{name}.log\n{text[-3000:]}')
    return text


def integrity():
    frozen = json.loads((AUTO/'frozen.json').read_text())
    baseline = json.loads((AUTO/'baseline.json').read_text())
    if digest(ART/'markers.so') != baseline['marker_sha256']:
        raise RuntimeError('frozen marker helper binary changed')
    for path, sha in frozen.items():
        if digest(ROOT/path) != sha:
            raise RuntimeError(f'frozen evaluator/input changed: {path}')
    changed = subprocess.check_output(['git','diff','HEAD','--name-only'], cwd=ROOT, text=True).splitlines()
    untracked = subprocess.check_output(['git','ls-files','--others','--exclude-standard'], cwd=ROOT, text=True).splitlines()
    if any(path not in ALLOW for path in changed) or untracked:
        raise RuntimeError(f'out-of-scope changes: changed={changed}, untracked={untracked}')


def build(tag):
    command(['bash','.auto/container.sh','cargo','build','--release','--locked','--offline',
             '--example','instruction_interpreter'],tag+'-build')
    if not BINARY.is_file():
        raise RuntimeError('missing interpreter harness binary')


def count(engine,case,n,samples,tag):
    extension = 'bxs' if engine == 'matchbox' else 'js'
    source = AUTO/'workloads'/f'{case}-{n}.{extension}'
    result = ART/f'{tag}-{engine}-{case}-{n}.json'
    add = {'control':0,'augment':2,'full':5}[case]
    expected = n*(n-1)//2 + n*add
    exe = [str(BINARY)] if engine == 'matchbox' else ['node','--jitless']
    args = ['python3','benchmarks/instructions/run.py','--mode','region',
            '--marker-library',str(ART/'markers.so'),'--samples',str(samples),'--timeout','30',
            '--input',str(source),'--input',str(BINARY) if engine == 'matchbox' else str(source),
            '--expect-last-line',str(expected),'--runtime-version',
            'current-source default-feature Rust harness; JIT inactive by assertion' if engine=='matchbox' else 'Node v22.23.3 --jitless',
            '--note','opt-level=z,lto=true,codegen-units=1; warmup100x1000+1ms; scoped all-thread instructions',
            '--output',str(result),'--',*exe,str(source)]
    command(args,f'{tag}-{engine}-{case}-{n}',seconds=samples*30+10)
    report = json.loads(result.read_text())
    if report['status'] != 'complete' or len(report['samples']) != samples:
        raise RuntimeError(f'incomplete counter report {result}')
    values = [s['instructions'] for s in report['samples']]
    median = statistics.median(values)
    return dict(median=median,mad=statistics.median(abs(x-median) for x in values),
                report=str(result.relative_to(ROOT)))


def measured(engine,tag):
    data = {}
    for case in ('control','augment','full'):
        samples = 5 if case == 'full' else 3
        small=count(engine,case,1000,samples,tag)
        large=count(engine,case,10000,samples,tag)
        slope=(large['median']-small['median'])/9000
        if slope <= 0:
            raise RuntimeError(f'nonpositive slope: {case}')
        data[case]=dict(instructions_per_iteration=slope,small=small,large=large,
                       noise_instructions_per_iteration=(small['mad']+large['mad'])/9000)
    return data


def parse_test_outcomes(text, suite):
    outcomes = {}
    pending = None
    for line in text.splitlines():
        if pending is not None:
            # One integration test reexecutes itself with inherited stdout. Its
            # nested runner is NOT the outer test result; wait for bare status.
            state = re.fullmatch(r'(ok|FAILED|ignored)(?:,.*)?', line)
            if state:
                outcomes[pending] = state[1]
                pending = None
            continue
        match = re.fullmatch(r'test (\S+) \.\.\.(?: (ok|FAILED|ignored)(?:,.*)?)?\s*', line)
        if match:
            if match[2]:
                outcomes[match[1]] = match[2]
            else:
                pending = match[1]
    totals = re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;', text)
    actual = tuple(sum(state == status for state in outcomes.values()) for status in ('ok','FAILED','ignored'))
    if pending or not outcomes or not totals or tuple(map(int,totals[-1])) != actual:
        raise RuntimeError(f'test identities do not match complete outer runner totals: {suite}')
    return outcomes


def tests(tag):
    identities = {}
    for suite, selection in SUITES.items():
        text=command(['bash','.auto/container.sh','cargo','test','--locked','--offline',
                      *selection,'--','--test-threads=1'],f'{tag}-{suite}',permit_test_failure=True)
        identities[suite]=parse_test_outcomes(text,suite)
    return identities


def probes(tag):
    command([str(BINARY),'.auto/workloads/original.bxs'],tag+'-original',seconds=30)
    if (ART/f'{tag}-original.log').read_text().strip() != '10':
        raise RuntimeError('original script wrong answer')
    for path in PROBES:
        command([str(BINARY),'tests/scripts/'+path],tag+'-probe-'+Path(path).stem,seconds=30)


def prepare():
    if (AUTO/'baseline.json').exists() or (AUTO/'log.jsonl').exists():
        raise RuntimeError('preparation baseline already exists; do not reset a campaign')
    tag='preparation-'+stamp()
    version=subprocess.check_output(['node','--version'],text=True).strip()
    if version != 'v22.23.3':
        raise RuntimeError(f'Node reference version changed: {version}')
    command(['python3','.auto/test_campaign.py'],tag+'-evaluator-tests')
    command(['python3','-m','unittest','discover','-s','benchmarks/instructions','-p','test_*.py'],tag+'-counter-tests')
    build(tag)
    probes(tag)
    primary=measured('matchbox',tag)
    node=measured('node',tag)
    identities=tests(tag)
    data=dict(tag=tag,matchbox=primary,node_jitless=node,tests=identities,
              binary_bytes=BINARY.stat().st_size,binary_sha256=digest(BINARY),
              marker_sha256=digest(ART/'markers.so'))
    write(AUTO/'baseline.json',data)
    print(json.dumps(data,indent=2))


def budget():
    if PREPARATION:
        return
    path=ART/'session-start.json'
    if not path.exists():
        with path.open('x') as out:
            json.dump(dict(start=time.time()),out)
    start=json.loads(path.read_text())['start']
    if time.time()-start >= 3600:
        raise RuntimeError('60-minute campaign budget exhausted; stop, do not reset')
    log=AUTO/'log.jsonl'
    if log.exists():
        runs=[json.loads(line) for line in log.read_text().splitlines() if line.strip()]
        if sum('run' in row for row in runs)>=10:
            raise RuntimeError('10-run campaign budget exhausted; stop')


def measure():
    integrity(); budget()
    tag='run-'+stamp()
    build(tag); budget()
    baseline=json.loads((AUTO/'baseline.json').read_text())
    data=measured('matchbox',tag); budget()
    for case in ('control','augment'):
        if data[case]['instructions_per_iteration']>baseline['matchbox'][case]['instructions_per_iteration']*1.05:
            raise RuntimeError(f'{case} exceeds 5% instruction regression guard')
    if BINARY.stat().st_size>baseline['binary_bytes']*1.05:
        raise RuntimeError('harness executable size exceeds 5% baseline guard')
    record=dict(tag=tag,measurement=data,binary_sha256=digest(BINARY),binary_bytes=BINARY.stat().st_size)
    write(ART/'last-measurement.json',record)
    write(ART/f'{tag}-summary.json',record)
    for case in ('control','augment','full'):
        key='instructions_per_iteration' if case=='full' else case+'_instructions_per_iteration'
        print(f'METRIC {key}={data[case]["instructions_per_iteration"]:.6f}')
    print(f'METRIC binary_bytes={BINARY.stat().st_size}')
    print(f'METRIC ratio_to_node_jitless={data["full"]["instructions_per_iteration"]/baseline["node_jitless"]["full"]["instructions_per_iteration"]:.6f}')


def checks():
    integrity(); budget()
    baseline=json.loads((AUTO/'baseline.json').read_text())
    current=json.loads((ART/'last-measurement.json').read_text())
    if digest(BINARY)!=current['binary_sha256']:
        raise RuntimeError('measured binary changed before checks')
    tag=current['tag']+'-checks'
    command(['python3','.auto/test_campaign.py'],tag+'-evaluator-tests')
    command(['python3','-m','unittest','discover','-s','benchmarks/instructions','-p','test_*.py'],tag+'-counter-tests')
    probes(tag)
    outcomes=tests(tag); budget()
    for suite,expected in baseline['tests'].items():
        actual=outcomes[suite]
        if actual.keys()!=expected.keys():
            raise RuntimeError(f'test identities changed: {suite}')
        for name,state in expected.items():
            if (state=='ok' and actual[name]!='ok') or (state=='ignored' and actual[name]!='ignored'):
                raise RuntimeError(f'baseline test regressed: {suite}/{name}: {state}->{actual[name]}')
            if actual[name]=='FAILED' and state!='FAILED':
                raise RuntimeError(f'new failure: {suite}/{name}')
    # Independent confirmation is mandatory, not merely advisory dashboard noise.
    confirm=measured('matchbox',tag+'-confirmation'); budget()
    score=current['measurement']['full']['instructions_per_iteration']
    again=confirm['full']['instructions_per_iteration']
    noise=max(current['measurement']['full']['noise_instructions_per_iteration'],confirm['full']['noise_instructions_per_iteration'])
    if abs(score-again)>max(score*.01,noise*3):
        raise RuntimeError('confirmation differs by >1% and >3x measured MAD-based noise')
    if BINARY.stat().st_size>baseline['binary_bytes']*1.05:
        raise RuntimeError('binary-size regression')
    for case in ('control','augment'):
        if confirm[case]['instructions_per_iteration']>baseline['matchbox'][case]['instructions_per_iteration']*1.05:
            raise RuntimeError(f'confirmation control regression: {case}')
    write(ART/f'{tag}-confirmation-summary.json',confirm)
    integrity()
    print('CHECKS PASSED: identities preserved, interpreter probes, frozen inputs, size/control guards, confirmation')


def dryrun():
    global PREPARATION
    if (AUTO/'log.jsonl').exists() or (ART/'session-start.json').exists():
        raise RuntimeError('dryrun is preparation-only; cannot bypass an active session budget')
    PREPARATION = True
    measure()
    checks()


if __name__=='__main__':
    ART.mkdir(parents=True,exist_ok=True)
    os.chdir(ROOT)
    if len(sys.argv)!=2 or sys.argv[1] not in ('prepare','measure','checks','integrity','dryrun'):
        raise SystemExit('usage: campaign.py prepare|measure|checks|integrity|dryrun')
    try:
        globals()[sys.argv[1]]()
    except Exception as error:
        print(f'CAMPAIGN FAILED: {error}',file=sys.stderr)
        raise SystemExit(1)
