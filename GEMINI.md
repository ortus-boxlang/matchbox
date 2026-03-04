# BoxLang Rust Implementation (bx-rust)

This project is a native Rust implementation of the [BoxLang](https://github.com/ortus-boxlang/BoxLang) programming language. It aims to provide a fast, JVM-independent runtime for BoxLang scripts (`.bxs`).

## Project Vision
To create a high-performance, standalone implementation of BoxLang that focuses on syntax compatibility and built-in functions (BIFs) without the overhead of the JRE.

## Architectural Decisions

### 1. Parser: Pest over ANTLR
- **Decision:** Use [Pest](https://pest.rs/) for grammar and parsing instead of the official ANTLR4 grammar.
- **Rationale:** The official BoxLang ANTLR grammar contains Java-specific actions and targets. `antlr-rust` is less idiomatic and harder to maintain for this specific use case. Pest allows for a clean, Rust-native PEG grammar that is easy to extend and debug.
- **Location:** `src/parser/boxlang.pest` and `src/parser/mod.rs`.

### 2. Execution: Tree-Walking Evaluator
- **Decision:** Use a tree-walking interpreter for the Proof of Concept.
- **Rationale:** Provides the quickest path to functional parity for dynamic language features (scoping, BIFs, dynamic typing) while maintaining a clean AST structure for future bytecode compilation or JIT improvements.
- **Location:** `src/evaluator/mod.rs`.

### 3. Scoping & Types
- **Decision:** Case-insensitive variable resolution and dynamic typing.
- **Rationale:** BoxLang/CFML heritage requires case-insensitivity. The `Environment` struct handles this by normalizing keys to lowercase. `BxValue` handles dynamic type switching.
- **Location:** `src/env/mod.rs` and `src/types/mod.rs`.

## Development Guidelines

### Adding New Syntax
1. Update `src/parser/boxlang.pest` with the new rule.
2. Add the corresponding variant to the `Statement` or `Expression` enum in `src/ast/mod.rs`.
3. Update the `parse_statement` or `parse_expression` functions in `src/parser/mod.rs` to map the Pest rule to the AST node.
4. Implement the execution logic in `src/evaluator/mod.rs`.

### Adding Built-In Functions (BIFs)
- Native BIFs are currently intercepted in `src/evaluator/mod.rs` inside `Expression::FunctionCall`.
- To add a BIF, add its name (lowercase) to the match arms and implement the logic using Rust functions.

## Future Roadmap
- [x] Implement `&` for string concatenation (converts operands to strings).
- [x] Implement anonymous functions, closures, and arrow (lambda) syntax.
- [x] Implement `return` statement and proper function return values.
- [x] Add support for `Array` and `Struct` (HashMap) types.
- [x] Implement the `for(item in collection)` loop for arrays and structs.
- [ ] Expand the library of BIFs (Math, String manipulation).
- [ ] Add a REPL mode.
