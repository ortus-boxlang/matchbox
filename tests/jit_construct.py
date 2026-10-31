#!/usr/bin/env python3
"""Compile the real BoxLang mapping loop, not a native mapping replacement."""
import os, subprocess, sys, tempfile
from pathlib import Path
app = Path(sys.argv[2]).resolve()
cli = Path(sys.argv[1]).resolve()
text = (app/'bifs/ast.bxs').read_text().split('function bxAstRange(',1)[0]
position = (app/'bifs/lint.bxs').read_text().splitlines()[1]
with tempfile.TemporaryDirectory(prefix='matchbox-jit-construct-') as directory:
    work = Path(directory)
    source = work/'mapping.bxs'
    source.write_text(position+'\n'+text+'''\nfunction checkMapping() {
    var source = repeatString("aé中😀" & chr(13) & chr(10), 3000);
    var first = bxTextMap(source);
    var second = bxTextMap(source);
    assert jsonSerialize(first) == jsonSerialize(second);
    assert len(first.chars) == 18000;
    assert len(first.positions) == 18001;
    assert len(first.bytes) == 36001;
    assert first.positions[18001].line == 3000;
    assert first.positions[18001].character == 0;
    assert first.bytes[36001] == 18001;
    first.positions[1].line = 999;
    assert second.positions[1].line == 0;
    assert bxTextMap("").positions[1].character == 0;
    println("Constructing JIT / real text map / UTF-8 and UTF-16 / CRLF / EOF / independent output: OK");
}
checkMapping();
function defaultRecord(first = 42, second = 0) { return {first: first, second: second}; }
function appendRecords(values, output) {
    var index = 0; var count = len(values);
    for (index = 1; index <= count; index++) { arrayAppend(output, defaultRecord(values[index], index)); }
    return count;
}
function appendBeforeRead(values, output) {
    var index = 0; var code = 0; var count = len(values);
    for (index = 1; index <= count; index++) { arrayAppend(output, index); code = ascii(values[index]); }
    return code;
}
function earlyReturn(count, output) {
    var index = 0;
    for (index = 1; index <= count; index++) { arrayAppend(output, index); if (index == 12000) { return 17; } }
    return 19;
}
function checkRollback() {
    var values = []; var output = []; var i = 0; var caught = false;
    for (i = 1; i <= 13000; i++) { arrayAppend(values, i); }
    values[12000] = null;
    assert appendRecords(values, output) == 13000;
    assert len(output) == 13000;
    assert output[12000].first == 42;
    values = reMatch(".", repeatString("x", 13000)); output = [];
    values[12500] = "";
    try { appendBeforeRead(values, output); } catch (any error) { caught = true; }
    assert caught;
    assert len(output) == 12500;
    assert output[12500] == 12500;
    output = [];
    assert earlyReturn(13000, output) == 17;
    assert len(output) == 12000;
}
checkRollback();
var savedAscii = ascii;
ascii = (value) => 77;
var reboundOutput = [];
assert appendBeforeRead(output = reboundOutput, values = reMatch(".", repeatString("x", 13000))) == 77;
assert len(reboundOutput) == 13000;
ascii = savedAscii;
defaultRecord = (first, second) => { return {first: first + 5, second: second}; };
var reboundValues = []; var reboundIndex = 0; reboundOutput = [];
for (reboundIndex = 1; reboundIndex <= 13000; reboundIndex++) { arrayAppend(reboundValues, reboundIndex); }
assert appendRecords(reboundValues, reboundOutput) == 13000;
assert reboundOutput[13000].first == 13005;
''')
    env = dict(os.environ, BOXLANG_HOME=str(work/'home'), MATCHBOX_JIT_TRACE='1', PATH='/usr/bin:/bin')
    def check(command):
        result = subprocess.run(command,cwd=work,env=env,capture_output=True,timeout=90)
        assert result.returncode == 0,(result.stdout,result.stderr)
        assert result.stdout.strip() == b'Constructing JIT / real text map / UTF-8 and UTF-16 / CRLF / EOF / independent output: OK',result.stdout
        assert b'[JIT] executed constructing loop fn name=bxTextMap' in result.stderr, result.stderr
    check([str(cli),str(source)])
    binary=work/'mapping'
    result=subprocess.run([str(cli),'--no-shaking','--target','native','--output',str(binary),str(source)],cwd=work,env=env,capture_output=True,timeout=90)
    assert result.returncode==0,result.stderr
    bytecode=work/'mapping.bxb'
    result=subprocess.run([str(cli),'--no-shaking','--build','--output',str(bytecode),str(source)],cwd=work,env=env,capture_output=True,timeout=90)
    assert result.returncode==0,result.stderr
    source.unlink();env['MATCHBOX_NATIVE_CACHE']=str(work/'cache')
    check([str(cli),str(bytecode)])
    check([str(binary)])
print('Source-free constructing JIT executes the actual BoxLang mapping loop: OK')
