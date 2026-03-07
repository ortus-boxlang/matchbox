# Differences from BoxLang (JVM)

MatchBox implements a **strict subset** of the BoxLang language. Code written for MatchBox will generally run unmodified on the main BoxLang JVM runtime. The reverse is not always true — JVM BoxLang has a much larger standard library, full Java interop, and additional language constructs that are not yet implemented in MatchBox.

This page documents the known differences so you can write portable code or understand why something that works on the JVM does not work in MatchBox.

---

## What Works the Same

The core language is fully compatible. These features behave identically in both runtimes:

- Variables, types, and operators
- String interpolation (`#variable#`)
- Control flow: `if`, `else if`, `else`, `for` loop, `for...in` loop
- Functions, closures, arrow functions, default arguments, type hints
- Arrays (1-indexed) and Structs (case-insensitive keys)
- Classes, inheritance (`extends`), implicit accessors (`accessors="true"`)
- Interfaces (including default implementations)
- `onMissingMethod` magic method
- Exception handling (`try`, `catch`, `finally`, `throw`)
- Async operations (`runAsync`, `sleep`, `Future.get()`)
- Member method delegation to BIFs (e.g., `"hello".ucase()`)

---

## Runtime & Environment

### No JVM Required

MatchBox is 100% JVM-independent. It does not use the JRE, load JARs, or run on the JVM in any form.

**Implication:** Any BoxLang feature that depends on the JVM — dynamic class loading, Java reflection, JMX, GroovyScript interop — is not available.

### No CFML / CFScript Compatibility Layer

The JVM BoxLang runtime includes extensive compatibility utilities for CFML. MatchBox does not include any CFML compatibility layer.

---

## Built-In Functions (BIFs)

The JVM BoxLang runtime ships with a very large standard library of BIFs covering strings, dates, arrays, structs, XML, JSON, database access, and more. MatchBox includes only a minimal prelude:

| BIF | MatchBox | BoxLang JVM |
| :--- | :--- | :--- |
| `println()` | ✅ | ✅ |
| `arrayLen()` | ✅ | ✅ |
| `arrayAppend()` | ✅ | ✅ |
| `arrayMap()` | ✅ | ✅ |
| `arrayToList()` | ✅ | ✅ |
| `abs()`, `min()`, `max()` | ✅ | ✅ |
| `runAsync()`, `sleep()` | ✅ | ✅ |
| Date / time functions | ❌ | ✅ |
| JSON encode / decode | ❌ | ✅ |
| File I/O BIFs | ❌ | ✅ |
| Database / query functions | ❌ | ✅ |
| Regular expressions | ❌ | ✅ |
| `createObject()` (Java) | ❌ | ✅ |

> **Workaround:** Missing BIFs can often be implemented in pure BoxLang and placed in the `prelude.bxs` or a shared include file, or implemented as [Native Fusion](building-and-deploying/native-builds.md#native-fusion-rust-interop) Rust BIFs.

---

## Java Interop

| Capability | MatchBox (Native) | MatchBox (WASM) | BoxLang JVM |
| :--- | :--- | :--- | :--- |
| `createObject("java", ...)` | ⚠️ Experimental JNI | ❌ | ✅ |
| Calling Java methods | ⚠️ Experimental JNI | ❌ | ✅ |
| Loading JARs at runtime | ❌ | ❌ | ✅ |
| Java generics / reflection | ❌ | ❌ | ✅ |

The experimental JNI bridge in native builds requires a compatible JDK installed on the target machine at runtime. It supports basic method invocation but does not handle generics, annotations, or dynamic class loading.

---

## Language Features Not Yet Implemented

These BoxLang language features exist in the JVM runtime but are not yet available in MatchBox:

| Feature | Status | Notes |
| :--- | :--- | :--- |
| `include` / `import` multi-file | ⚠️ Partial | `import` works for classes; `include` not fully supported |
| Switch / match statements | ❌ | Use `if/else if` chains |
| `cfhttp` / HTTP requests | ❌ | Use Native Fusion + a Rust HTTP crate |
| Template / tag syntax | ❌ | Script syntax only |
| Query of Queries | ❌ | No database layer |
| Component (`cfc`) files | ❌ | Use `.bxs` class files |
| ORM / Hibernate | ❌ | No persistence layer |
| `writeDump()` / debugging | ❌ | Use `println()` |
| Full regex support | ❌ | Not yet implemented |

---

## Scoping Differences

The JVM BoxLang runtime supports the full scope chain: `application`, `session`, `request`, `cgi`, `url`, `form`, and `cookie` scopes. MatchBox is a standalone runtime with no web server and therefore does not provide these web-specific scopes.

Supported scopes in MatchBox:

| Scope | Description |
| :--- | :--- |
| `this` | Current component / class instance |
| `variables` | Component-private variable scope |
| `local` / `var` | Function-local variable scope |
| arguments | Function argument scope |

---

## Type System

Both runtimes use dynamic typing, but the JVM runtime uses Java types under the hood and can interact with strongly-typed Java APIs. MatchBox uses its own value type system (`BxValue`) which covers:

- `null`
- `boolean`
- `number` (f64)
- `string`
- `array`
- `struct`
- `closure` / `function`
- `future`
- `class instance`

There is no `byte`, `int`, `long`, `Date`, `BigDecimal`, or other JVM-specific type in MatchBox.

---

## Concurrency Model

The JVM BoxLang runtime uses Java threads. MatchBox uses a **cooperative fiber scheduler** — a single-threaded event loop where `runAsync` tasks yield via `sleep` or I/O, not via OS thread preemption.

**Practical implications:**

- CPU-bound loops do not automatically yield. If you run a long computation inside `runAsync`, other fibers will not get CPU time until it finishes.
- You cannot use Java `synchronized` or `volatile` constructs — they do not exist in MatchBox.
- Concurrency overhead is extremely low compared to threads.

---

## Writing Portable Code

To maximize compatibility with the JVM BoxLang runtime:

1. **Avoid Java-specific BIFs** — don't use `createObject("java", ...)` or rely on JVM classes.
2. **Stick to core BIFs** — prefer functions that exist in both runtimes.
3. **Use `.bxs` for classes** — avoid `.cfc` or BL-template syntax.
4. **Test on both runtimes** — MatchBox uses a high-performance integration test suite; the JVM has its own.
5. **Keep scoping simple** — use `var` for locals and avoid web-specific scopes.

Code that follows these rules will run identically on both MatchBox and the JVM runtime.
