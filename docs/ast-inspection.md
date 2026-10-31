# AST inspection

`matchbox-parser` owns the existing lexer, script/template compiler AST, parser and CST. Both `matchbox-compiler` and `matchbox-vm` depend on it. There is one frontend, no compiler/VM dependency cycle, and parsing does not execute inspected code.

## BoxLang contract

```boxlang
ast = boxAST(source="function add(a,b) { return a+b; }");
ast = boxAST(filepath="src/Example.bx");
json = "x=1+2".toAST(returnType="json");
editor = boxAST(source="var good=1; var broken=;", recover=true, includeTokens=true);
```

Arguments retain the JVM order: `source`, `filepath`, `returnType="struct"`, `sourceType="script"`. MatchBox adds Boolean `recover=false` and `includeTokens=false`. Nonblank source takes precedence over filepath. File input requires `bif-io`; `.bxm` is recognized as template input. Strict blank/no input fails; editor recovery accepts an empty document.

- `struct`: a real nested AST rooted at `BoxScript`.
- `json`: the same exported data as JSON.
- `text`: original parsed source text; not a formatting/PrettyPrint implementation.
- Strict parsing throws parse/lexical errors. `recover=true` preserves valid statements inside their original function/class/block scopes, including unclosed bodies and adds `issues` containing message, filename, position and span. It does not fabricate a successful complete tree for malformed input.
- `includeTokens=true` adds source-backed `tokens` and `trivia` (whitespace and comments). Comments are also collected on the root; JVM comment-to-node attachment is not reproduced.

Class properties retain shorthand and valued annotations (including types/defaults), their individual key/value spans, and complete property source spans. Inspection does not evaluate defaults; compilation initializes property defaults rather than treating metadata as executable sibling statements.

Nodes use `ASTType`, `ASTPackage`, `sourceText`, `comments` and node-specific fields. The exporter is separate from internal Rust enum layouts. Familiar JVM node names/fields are used where retained by the compiler; compiler-only forms use explicit `MatchBox*` names. This is a **bounded exported schema, not exact JVM AST parity**.

## Positions

- `position.start/end`: **one-based lines, zero-based UTF-16 columns**, end exclusive. LSP adapters subtract one from lines.
- `span.start/end`: **zero-based UTF-8 byte offsets**, end exclusive, into the supplied/read source.
- Recovery marks nodes containing unclosed blocks with `incomplete: true` and extends their spans through EOF trivia. Closed children are not marked; editor scope lookup can include the EOF cursor only for incomplete scopes.
- Class/function declarations also export `bodySpan` (UTF-8 byte offsets, including braces), distinct from declaration/header/default-expression spans. Unclosed bodies extend through EOF trivia. This is an inspection extension, not a JVM schema claim.
- Source-backed script statements/expressions, tokens and trivia have spans. Compiler-synthesized/desugared subnodes without retained spans omit position/span and have null sourceText rather than guessed coordinates. Interpolation expression offsets are translated back into the containing source.
- Internal lexer `Span.col` remains one-based Unicode-scalar indexing. Consumers must not confuse it with the exported UTF-16 column.

## Experimental native snapshots and BoxLang visitors

MatchBox adds `returnType="native"`; JVM `boxAST` does not support it. This retains
an owned typed Rust parse and a compact syntax index, without building the JSON
intermediate or VM AST structs. It does not move analysis/diagnostics into Rust.

```boxlang
function visit(node, phase, state) {
    if (phase == 0) { arrayAppend(state.names, node.typeName()); }
    return 0;
}
tree = boxAST(source="var answer=42;", returnType="native", recover=true);
walk = boxASTVisit(tree=tree, visitor=visit, state={names: []});
```

`boxASTVisit(tree, visitor, state)` accepts a snapshot root or retained node and a
function or BoxLang object with `visit(node, phase, state)`. Normal BoxLang argument,
exception, capture and method semantics apply. Callback throws preserve their
original type, message, fields and stack (including typed catches), rather than
being rendered into a new ExpressionException. Reentrant traversal and traversal
after a handled callback error are supported. Phases are **0 enter / 1 leave**.
Numeric actions are **0 continue / 1 skip children / 2 stop**; other values fail.
Skipped nodes still receive leave; stop and exceptions do not synthesize unwinding
leave events. Walk order follows the defined native syntax edges, not exported
metadata-key order. Result: `{state, completed, stoppedAt}`; state is the caller's
mutable analysis state, stoppedAt retains its source snapshot. No inspected source
executes. Traversal is synchronous; this is not async cancellation.

Views expose readonly scalar `kind`, `id`, `parentId`, `bodyId`, `childCount`,
`start`, `end`, `incomplete`. IDs are one-based and snapshot-local; absent parent/body
is 0, absent spans are null. Dot/index assignments throw. Methods:

- `typeName()`, `name()`, `returnType()`, `edge()`, `text()` are lazy strings/null.
- `kinds()` returns an independent name-to-numeric-kind map; consumers need not
  hardcode discriminants. Native kind names describe the compiler syntax, **not**
  the BoxLang exported AST schema. This is a bounded inspection view, not every
  original syntax field or a type-inferred semantic tree.
- `nodes([kindName])` selects nodes across the whole snapshot; `node(id)` reads an
  explicit snapshot-local node. Function/closure/lambda bodyId selects the body.
- `position()` uses the same one-based-line/UTF-16-column convention; synthesized
  nodes without source spans return null. Source spans stay UTF-8 byte offsets.
- Root `toStruct()` exports the retained parse without reparsing, including current
  recovery issues/tokens when requested. It deliberately materializes the legacy
  representation; this is a bridge, not a zero-copy migration. Exports are independent.

Native views own no VM values/IDs internally and remain valid across GC or loss of
the original root. Traversal roots callable/state/node values across callbacks and
releases roots on ordinary errors. No foreign-VM values enter shared program caches.

The existing guarded readonly JIT can read opted-in native **scalar** properties.
The internal `BxNativeObject::readonly_jit_property` contract is pure, allocation-
free, callback-free and returns no GC values; all other native getters stay
interpreted. This is not a new plugin ABI capability. Callback bodies still need
ordinary JIT eligibility; mutable collectors, arbitrary calls and many fused/control
forms remain interpreted. A native driver alone does not establish a compiled visitor.

Checks: `vm_ast_visit` integration and application `tests/native_ast.py CLI RUNTIME_ROOT`
cover function/class callbacks, actual visitor native execution, canonical export,
readonly views, retained ownership, original function/class callback exceptions,
reentrant/post-error traversal and source-free bytecode/standalone packaging.

## Current boundaries

The compiler AST loses some original syntax: compound assignments and some unary forms are desugared; assignment targets, parameters, argument names, declaration annotations and other synthesized nodes do not yet all retain individual spans. Recovery skips invalid statements at balanced boundaries and reports missing closing braces without inventing tokens. Incomplete declaration headers and malformed class properties are not yet retained. Full source-preserving node/schema parity requires further frontend work.

`sourceType="template"`, `"cfscript"` and `"cftemplate"` currently fail explicitly. In particular, the template compiler currently fabricates loop/condition/function data; exposing those results as faithful analysis would be incorrect. Existing template compilation remains available and is not certified by the inspection API.

Checks: `cargo test -p matchbox_parser` and `cargo test --test integration_tests vm_box_ast`. The public non-extension BoxLang contract in `tests/scripts/vm_box_ast.bxs` is also exercised against the JVM reference.
