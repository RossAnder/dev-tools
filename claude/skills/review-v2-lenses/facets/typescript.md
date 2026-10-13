+++
id = "typescript"
status = "stable"
families = ["code"]
apply_when = { signals = ["lang:ts"] }
attaches_to = ["type-design", "correctness", "idioms"]
+++

# typescript

## General

Read the nearest `tsconfig.json` and what it extends before judging any type. `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes` and `verbatimModuleSyntax` change what the compiler already catches, and a probe the config already enforces is not a finding. Read `package.json` for the TypeScript version and the runtime (browser bundle, Node, Bun) before citing an API.

- Types are erased at runtime. Every value that arrives from `JSON.parse`, `fetch(...).json()`, `localStorage`, a WebSocket frame or `postMessage` is `any` or `unknown` until something checks it. An `as T` on that value is a claim, not a check.
- `as`, `!` and `// @ts-expect-error` each switch the compiler off at one site. Count them in the slice and read each one; one that asserts a shape the data can violate is where runtime errors come from.
- `number` is a double. An integer id or counter from a 64-bit backend (Rust `i64`/`u64`, C# `long`) loses precision past 2^53 when `JSON.parse` reads it.
- `||` treats `0`, `""` and `false` as missing; `??` does not. A default written with `||` on a field where zero or empty is valid is wrong.
- Structural typing accepts extra properties wherever a value is not an object literal, so spreading or passing a wider object can carry fields a narrower type does not declare onto the wire.

## By lens

### For type-design

- An object type with several optional fields whose valid combinations depend on a `status` or `kind` field is a candidate for a discriminated union keyed on that field, so each variant carries only its own fields.
- Ids from different tables typed as bare `string` or `number` are interchangeable to the compiler. Where the slice passes two kinds of id through one signature, a branded type (`string & { readonly __brand: "TaskId" }`) makes a swapped argument fail to compile.
- A closed set typed as `string` should be a literal union, or an `as const` array with its union derived from it. Where the codebase validates at the boundary with a schema library (zod, valibot, io-ts), the schema should be the single source and the type derived from it (`z.infer<typeof Schema>`); a hand-written interface beside the schema can drift from it silently.
- A wire mirror of a backend enum or struct must reject or surface a value it does not know. A `z.enum` rejects unknown values hard and a `z.object` strips unknown keys by default; check which behaviour the call site needs when the backend adds a variant or field.
- `Record<string, T>` and index signatures return `T`, not `T | undefined`, unless `noUncheckedIndexedAccess` is on, so a lookup that can miss is typed as if it always hits. A `Map` or an explicit `| undefined` states the miss.
- A `switch` over a union should end in an exhaustiveness check (`const _exhaustive: never = value`) so adding a variant fails the build at every unhandled site.

### For correctness

- A promise that is neither awaited nor returned nor given a `.catch` drops its rejection. Check `async` callbacks passed to `forEach`, `map` without `Promise.all`, event handlers and `setTimeout`; `forEach` never awaits its callback.
- `catch (e)` binds `unknown` under strict settings; code that reads `e.message` after an `as Error` mishandles a thrown string or a rejected non-Error.
- `Array.prototype.sort` sorts in place and, with no comparator, compares as strings, so `[10, 9, 1].sort()` gives `[1, 10, 9]`. A comparator that returns a boolean instead of a signed number sorts inconsistently across engines. Sorting an array the caller still holds mutates the caller's data; `toSorted` avoids it where the target supports it.
- `new Date("2026-10-12")` parses as UTC midnight and `new Date("2026-10-12T00:00")` as local midnight; `getMonth()` is zero-based. Date arithmetic across a DST change by adding `86_400_000` lands an hour off.
- `Number("")` is `0`, `parseInt("08abc")` is `8`, and `Number.isNaN` and the global `isNaN` differ on non-numbers. Parsing user or wire input this way accepts what should be rejected.
- `JSON.parse` throws on malformed input; a call on a network or storage value with no `try` turns bad data into an unhandled crash.
- A fetch or async load started by a changing input needs cancellation or a staleness check (`AbortController`, a request token), or a slower earlier response overwrites a newer one.
- `for...in` over an array iterates string keys and inherited enumerable properties; `Object.keys` orders integer-like keys numerically first, whatever the insertion order.

### For idioms

- Follow the codebase's boundary pattern: if other modules validate wire data through a schema, a new module that casts with `as` diverges from it.
- Check the `lib` and `target` in tsconfig before suggesting `Array.prototype.at`, `toSorted`, `structuredClone` or `Object.groupBy`, and before flagging a hand-written helper that one of them replaces.
- `enum` versus literal unions, `interface` versus `type`, default versus named exports: decide from what the repo already converged on, with the history-idiom method, not from preference.
- With `verbatimModuleSyntax` or `isolatedModules` on, type-only imports need `import type`; a mixed import that the bundler cannot elide pulls a module in for its types.
- `satisfies` checks a value against a type without widening it; an annotation that widens an `as const` table and loses its literal keys is the pattern it replaces.
- Utility types reimplemented by hand (`Partial`, `Pick`, `Omit`, `ReturnType`, `Awaited`) are cleanup where the built-in states the same thing.
