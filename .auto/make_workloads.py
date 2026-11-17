#!/usr/bin/env python3
"""Preparation-only fixture generator; outputs are frozen before the campaign."""
from pathlib import Path
ROOT = Path(__file__).resolve().parent
BODIES = {
    'control': 'sum += i;',
    'augment': 'var x=i; x+=2; sum+=x;',
    'full': 'var x=i; x+=2; var y=add(x,3); sum+=y;',
}
for case, body in BODIES.items():
    for n in (1000, 10000):
        for engine, ext in [('matchbox', 'bxs'), ('node', 'js')]:
            log = 'println' if engine == 'matchbox' else 'console.log'
            idle = 'sleep(1);' if engine == 'matchbox' else 'Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,1);'
            source = f'''function add(a,b) {{ return a+b; }}
function batch(n) {{
    var sum=0;
    for (var i=0; i<n; i++) {{ {body} }}
    return sum;
}}
{log}("PRIMED");
for (var w=0; w<100; w++) {{ batch(1000); {idle} }}
{log}("__ICOUNT_START__");
var result=batch({n});
{log}("__ICOUNT_END__");
{log}(result);
'''
            path = ROOT / 'workloads' / f'{case}-{n}.{ext}'
            if path.exists() and path.read_text() != source:
                raise SystemExit(f'refusing to overwrite changed fixture {path}')
            path.write_text(source)
for engine, ext in [('matchbox', 'bxs'), ('node', 'js')]:
    log = 'println' if engine == 'matchbox' else 'console.log'
    (ROOT / 'workloads' / f'original.{ext}').write_text(f'''x=5;
x+=2;
function add(a,b) {{ return a+b; }}
y=add(x,3);
{log}(y);
''')
