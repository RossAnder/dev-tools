# Language cores

Loaded from `SKILL.md` when writing doc comments in Rust, C#, TypeScript, React, or Vue.

- [Where the type system replaces prose](#where-the-type-system-replaces-prose)
- [Rust](#rust)
- [C# / .NET](#c--net)
- [TypeScript](#typescript)
- [React](#react)
- [Vue](#vue)
- [The lint ladder](#the-lint-ladder)

## Where the type system replaces prose

Delete the comment when the signature already says it.

| C# | Rust | TypeScript |
|---|---|---|
| `string?` under NRT → "may be null" | `Option<T>` → "may be absent" | typed props → `@param` |
| `required` → "must be set before use" | `Result<T, E>` → "returns an error" | union type → "one of…" |
| `record` → "value equality / immutable" | `&mut self` → "mutates" | `readonly` → "not mutated" |
| `[Obsolete("use X")]` → prose deprecation | `#[must_use]` → "do not ignore the return" | `@deprecated` + `no-deprecated` |
| `IAsyncEnumerable<T>` → "streams results" | `Send` / `Sync` → **all thread-safety prose** | `Promise<T>` → "async" |
| `readonly struct` → "does not mutate" | ownership + `Drop` → the whole disposal genre | — |

**What none of them carry:** units and ranges (`timeout: u64` — millis or seconds?),
invariants spanning fields, allocation and blocking behaviour, ORM and serialisation
constraints, and *why* a value is what it is.

**Document the condition, not the nullability.** Under NRT, "may be null" is redundant but
"null if authentication fails" is not — the distinction is whether the prose names a failure
mode. A naive "delete null prose" rule destroys information.

## Rust

- One-line summary in third-person present indicative ("Returns…"), blank line, then detail.
- Sections (`# Examples`, `# Panics`, `# Errors`, `# Safety`) are **conditional on the
  behaviour existing**. Never emit an empty scaffolding section.
- **`# Safety` on every public `unsafe fn`** — the only doc obligation enforced by default in
  either ecosystem. It is not description, it is the proof obligation transferred to the
  caller.
- **Uppercase `// SAFETY:` immediately above every `unsafe {}` block**, naming the invariant
  that makes it sound. A safety comment on safe code is itself a defect.
- **Doc-tests are the strongest anti-drift mechanism available.** Add one when the example
  shows a call sequence or an invariant a caller could get wrong. `no_run` compiles without
  executing and still catches API drift; **`ignore` compiles nothing and catches nothing** —
  never use it without a bracketed reason.
- Use hidden `#` setup lines so the rendered example is the minimum a reader must understand;
  prefer `# Ok::<(), Error>(())` to `unwrap()`.
- `todo!()` = intended, not yet written. `unimplemented!()` = will not be written for this
  type. `unreachable!()` = cannot occur. Pick deliberately; they document different things.
  `todo!()` must not survive the change that makes its path reachable.
- **Do not use `#[doc(cfg(...))]`** — still nightly-gated on stable. State the feature
  requirement in prose.
- Enabling `missing_docs` implicitly mandates a crate-level `//!` block.

## C# / .NET

- **`GenerateDocumentationFile` and CS1591 are one decision, not two.** Without the doc file,
  CS1572, CS1573 and CS1574 — the *drift detectors* — cannot fire in any configuration.
  Check for the Debug-only-`false` footgun: it reads as a decision and is a no-op if Release
  never sets it true.
- **Never suppress CS1572 / CS1573 / CS1574.** These fire only on real drift — a `<param>`
  naming a parameter that no longer exists, partial param coverage after a signature change,
  an unresolvable `cref`. Promote them to `error`. Suppressing CS1573 disables the one warning
  that catches a renamed parameter.
- **CS1591 is the other one.** It tracks *effective* accessibility, so an internal-by-default
  application is already exempt. Enforce it only on a shipped public surface; suppress it
  conditionally elsewhere. One positional record emits five CS1591 warnings.
- **Delete any `<param>` whose body is the de-camel-cased parameter name.** If a parameter
  needs no constraint, unit, or ownership note, it needs no tag.
- **Use `[Obsolete("use X")]`, not prose.** CS0618/0619 is compiler-enforced; a `<summary>`
  saying "deprecated" is not.
- `<inheritdoc/>` on interface implementations and overrides rather than copying the base.
- Inline `<code>` in XML comments is **never compiled and rots silently**. Keep runnable
  examples in a compiled sample project referenced via `<include>`.
- Earn-their-place tags: `<summary>`, `<param>`, `<typeparam>`, `<exception cref>`,
  `<see cref>`, `<inheritdoc>`. Formatting tags (`<para>`, `<list>`, `<b>`) are noise in
  application code.

## TypeScript

- **A doc comment must add what the type does not.** `@param`/`@returns` are justified only
  when they carry a unit, range, invariant, or side effect. Blanket coverage manufactures
  signature echo at scale.
- Never write types in JSDoc braces in a `.ts` file.
- **`@throws` is unrepresentable in the type system** — document it on anything that can throw
  or reject.
- `@deprecated` names the replacement and the removal version.
- Release tags (`@public`/`@beta`/`@alpha`/`@internal`) are code wearing a comment's clothes —
  they drive `.d.ts` rollup trimming. Required in a published package, meaningless in an
  application.
- Every `@ts-expect-error` carries a description naming the cause and the removal condition.
- **Tooling trap:** `eslint-plugin-jsdoc`'s `recommended-typescript` leaves `require-param`,
  `require-param-description`, `require-returns` and `require-returns-description` **on** —
  only the `-type` variants are disabled. Turn those four off in an application.
  `jsdoc/informative-docs` is the anti-restatement rule and is in **no** preset — enable it
  deliberately. `eslint-plugin-tsdoc` ships one rule (`tsdoc/syntax`); it is syntax
  validation, not coverage.

## React

- Props are documented by the props type, not by `@param`. Add a line to a prop only for a
  constraint or default the type omits.
- Every `useEffect` gets a comment naming **the external system it synchronises** — or is
  refactored away instead. A comment listing the dependency array is noise; a comment
  explaining a *deliberately omitted* dependency is not.
- Under React Compiler, the comment that earns its place is **why the compiler was
  overridden** — an imperative library boundary, an external event system, a measured hotspot
  — with a removal condition. "Memoised for performance" with no measurement is cargo cult.
- Storybook autodocs is a published doc surface, not a default. Do not add it for a component
  with no external consumer.

## Vue

- Use type-based `defineProps<T>()`; document a prop only where the type leaves a constraint
  or default unstated.
- Document a composable's returned bindings' **reactivity contract** (ref vs computed vs
  readonly) and its lifecycle/cleanup requirements — none of which the type carries.
- Measure comment share on `<script>` blocks only; `<template>` and `<style>` make a
  whole-file line share meaningless.

## The lint ladder

Enable by stage. Earlier is not better — a coverage lint at S1 produces one-line restatements
of the signature, which is worse than nothing.

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

Generate any lint table from the tool's own source or `--print-config`, never from a scraped
docs page — rendered lint indexes misreport groups.
