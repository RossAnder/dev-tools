+++
id = "react"
status = "stable"
families = ["code", "perf"]
apply_when = { signals = ["framework:react"] }
attaches_to = ["memory-runtime", "algorithm", "correctness", "idioms"]
+++

# react

## General

Read the React version and the build setup before judging anything: `react` and `react-dom` in `package.json`, whether the React Compiler (`babel-plugin-react-compiler`) is configured, whether `StrictMode` wraps the root, and whether the app is a client SPA or uses Server Components (a `"use client"` directive marks the boundary). Advice about memoization, refs and effects differs across all four.

- A component body runs on every render and must be pure. Anything it does besides computing JSX — a fetch, a subscription, a mutation of a value it did not create — runs again on each render, and twice in development under `StrictMode`.
- Under `StrictMode` in development, React runs effect setup, cleanup, then setup again, and double-invokes component bodies and `useMemo` and `useState` initialisers. A double request or a doubled listener seen only in development is a missing cleanup or an impure body, not a React bug.
- In React 19, `ref` is an ordinary prop and `forwardRef` is no longer needed; a ref callback may return a cleanup function, and one that does not is called with `null` on detach.
- With the React Compiler on, components and hooks are memoized automatically; hand-written `useMemo`, `useCallback` and `React.memo` add little, and code that breaks the rules of React is skipped by the compiler rather than optimised.

## By lens

### For memory-runtime

- An effect that starts an interval, timer, subscription, event listener, observer or socket and returns no cleanup keeps the closure, and everything it captures, alive after unmount. Check every `useEffect` and `useLayoutEffect` in the slice for a returned cleanup that undoes its setup.
- A context provider whose `value` is an object or function literal built in render gives every consumer a new value on every render of the provider, so every consumer re-renders. The cost is the consumer count multiplied by the provider's render frequency.
- A long list rendered in full mounts one component instance per row. Past a few hundred rows that are mostly off screen, the instance count, not the row markup, is the cost; virtualisation renders only the visible window.
- Bundle size: a whole-package import of a large library (lodash, moment, date-fns locales, an icon set, a charting library) where a per-function or per-icon import exists; a heavy route, editor or chart loaded eagerly where `React.lazy` with `Suspense` would split it out.
- Tree-shaking barriers: a dependency that ships only CommonJS, a package without `"sideEffects": false`, and a barrel `index.ts` with `export *` from modules that run code on import each keep unused code in the bundle. Confirm with the bundle analyser or the build's chunk report before claiming a size.
- Large immutable data, such as a parsed API response, kept in state and replaced wholesale is fine; copying it on every update with spread to change one field allocates the whole object per update.

### For algorithm

- Reconciliation matches children by `key`. An index key in a list that can reorder, filter or insert makes React reuse the wrong component instances, and a key generated in render (`Math.random()`, `crypto.randomUUID()`) remounts every row on every render.
- State held high in the tree and updated per keystroke or per pointer move re-renders the whole subtree under it. Check where fast-changing state lives against which components read it.
- An expensive derivation in render (filter, sort, group over a large array) re-runs on every render, including renders caused by unrelated state. Without the compiler, `useMemo` keyed on its true inputs avoids it; a dependency that is a new object or array each render makes the memo recompute anyway.
- An inline object, array or function passed to a `React.memo` child is a new reference each render, so the memo never skips.
- A chain of effects where each sets state that triggers the next costs one render pass per link. Data derivable from props or state belongs in render, not in an effect plus a `useState`.
- A `setState` called in a loop where one update over the final value would serve; a search inside a row component over the whole list, making a list render quadratic.

### For correctness

- A stale closure: an effect, callback or interval that reads state or props omitted from its dependency array sees the values from the render that created it. A suppressed `react-hooks/exhaustive-deps` warning is where to look first.
- A fetch in an effect with no abort or ignore flag lets a slow earlier response overwrite a newer one when the input changes, and sets state after unmount.
- Mutating state in place (`items.push(x); setItems(items)`) passes the same reference, so React may skip the update and memoized children show stale data.
- An update computed from the previous value written as `setCount(count + 1)` loses updates when several run in one batch; the updater form `setCount(c => c + 1)` does not.
- `useState(props.value)` reads the prop once; a later prop change is ignored unless the component is keyed on it or the value is derived in render.
- An input switching between controlled and uncontrolled (`value` going from `undefined` to a string) loses its state and warns.
- Hooks called conditionally, in a loop or after an early return break the call order React relies on.
- With an index key, per-row state and uncontrolled inputs follow the index, not the item, after a delete or reorder.

### For idioms

- An effect used to derive data from props or state, or to react to a user event, where computing in render or handling it in the event handler would do; the React docs' "You Might Not Need an Effect" is the reference.
- Hand-written memoization in a codebase where the React Compiler is on; `forwardRef` in a React 19 codebase where `ref` as a prop is the converged form.
- Follow the data-fetching and state patterns the repo already uses (a query library, context, a store); a new component that fetches in a bare effect beside query-library hooks diverges from it.
- A class component or a `defaultProps` on a function component in a codebase of function components.
