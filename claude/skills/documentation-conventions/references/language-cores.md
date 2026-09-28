# Language cores

Loaded when writing doc comments in Rust, C#, TypeScript, React or Vue. It covers only what the type system and the defaults miss.

## Document the condition, not the type

The type already says a value may be absent, may fail, is async, or is immutable, and prose repeating it is signature echo. No type system carries units and ranges, invariants spanning fields, allocation and blocking behaviour, serialisation constraints, or why a value is what it is. Under C# nullable reference types, "may be null" is redundant but "null if authentication fails" is not: keep prose that names a failure mode.

## Rust

- Sections (`# Examples`, `# Panics`, `# Errors`, `# Safety`) appear only when the behaviour exists; never an empty scaffold.
- `// SAFETY:` above every `unsafe {}` block names the invariant that makes it sound. A safety comment on safe code is itself a defect.
- Doc-tests are the strongest anti-drift tool. `no_run` still compiles and catches API drift; `ignore` compiles nothing, so never use it without a bracketed reason.
- Enabling `missing_docs` implicitly mandates a crate-level `//!` block.

## C# / .NET

- `GenerateDocumentationFile` and CS1591 are one decision. Without the doc file, the drift detectors CS1572, CS1573 and CS1574 cannot fire in any configuration, and a Debug-only `false` reads as a decision but is a no-op if Release never sets it `true`.
- Never suppress CS1572/1573/1574: they fire only on real drift (a `<param>` for a parameter that no longer exists, partial coverage after a signature change, an unresolvable `cref`). Promote them to `error`.
- Enforce CS1591 only on a shipped public surface. It tracks effective accessibility, and one positional record emits five warnings.
- Use `[Obsolete("use X")]`, not prose deprecation. Inline `<code>` in XML docs is never compiled; keep runnable examples in a compiled sample referenced via `<include>`.

## TypeScript

- `@param` and `@returns` only when they carry a unit, range, invariant or side effect. No types in JSDoc braces in a `.ts` file.
- `@throws` is unrepresentable in the type system: document it wherever something throws or rejects.
- Release tags (`@public`, `@beta`, `@alpha`, `@internal`) drive `.d.ts` rollup trimming. Required in a published package, meaningless in an application.
- Every `@ts-expect-error` names the cause and the removal condition.
- `eslint-plugin-jsdoc`'s `recommended-typescript` leaves `require-param`, `require-param-description`, `require-returns` and `require-returns-description` on; only the `-type` variants are off. Turn those four off in an application. `jsdoc/informative-docs`, the anti-restatement rule, is in no preset, so enable it deliberately.

## React and Vue

- Every `useEffect` comment names the external system it synchronises, or the effect is refactored away. Explain a deliberately omitted dependency; never list the dependency array.
- Under React Compiler, the comment that earns its place is why the compiler was overridden, with a removal condition.
- A Vue composable documents its returned bindings' reactivity contract (ref, computed or readonly) and its cleanup requirements. Measure comment share on `<script>` blocks only.

## The lint ladder

Enable by stage. A coverage lint at S1 produces one-line restatements of the signature, which is worse than nothing.

| Setting | Eco | Enable at | Cost |
|---|---|---|---|
| `clippy::missing_safety_doc` (warn by default) | Rust | always — never allow | none |
| `rustdoc::broken_intra_doc_links` = deny | Rust | always, once clean | ~none |
| `clippy::undocumented_unsafe_blocks` | Rust | at the second `unsafe` block | retrofit churn |
| `#![warn(missing_docs)]` | Rust | S3 / first publish | forces a crate-level `//!` |
| `#![deny(missing_docs)]` | Rust | post-1.0 published | blocks merges on stubs |
| `clippy::missing_errors_doc`, `missing_panics_doc` (pedantic) | Rust | at publish | noisy on `Result`-heavy internals |
| `clippy::doc_markdown` (**pedantic**, not style) | Rust | published crate | needs a `doc-valid-idents` allowlist |
| `clippy::todo`, `clippy::unimplemented` | Rust | **pre-release gate only** | blocks scaffolding — enable late |
| `GenerateDocumentationFile` | C# | wherever drift detection is wanted | the XML write |
| CS1572/1573/1574 = `error` | C# | whenever the doc file is on | none — only fire on real drift |
| CS1591 unsuppressed | C# | shipped public surface only | 5 warnings per positional record |
| `jsdoc/informative-docs` | TS | always (in no preset) | none |
| `jsdoc/require-param`, `require-returns` | TS | **off in applications** | manufactures echo |
| `@typescript-eslint/ban-ts-comment` (strict) | TS | always | none |

Generate any lint table from the tool's own source or `--print-config`, never from a scraped docs page: rendered lint indexes misreport groups.
