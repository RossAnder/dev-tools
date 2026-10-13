+++
id = "vue"
status = "stable"
families = ["code", "perf"]
apply_when = { signals = ["framework:vue"] }
attaches_to = ["memory-runtime", "algorithm", "correctness", "idioms"]
+++

# vue

## General

Read the Vue version in `package.json` and how the app is mounted before judging anything. `createApp` is the virtual-DOM runtime; `createVaporApp` is Vapor mode, where every SFC under the root is compiled with `<script setup vapor>` and a VDOM-compiled component needs `createApp` plus `vaporInteropPlugin` to mount alongside. Check whether the codebase uses the Composition API, the Options API or both, and what its state pattern is (Pinia, module-singleton composables, provide/inject).

- `ref()` and `reactive()` make their contents deeply reactive: every nested object is wrapped in a proxy on access, and every property read inside a computed, watcher or render is tracked. That cost is proportional to the size of the data, not to what the view reads.
- A composable that registers a watcher, listener, interval or subscription is cleaned up only if it runs inside an effect scope (a component's `setup` or an `effectScope`) and registers its teardown with `onScopeDispose` or `onUnmounted`. Module-level state in a composable lives for the page's lifetime.
- Code after an `await` inside a composable, or inside a hand-written `setup()` function, has lost the current instance: `onUnmounted`, `onScopeDispose`, `getCurrentScope()` and `inject` called there do not attach to the component. `<script setup>` restores the instance only around its own top-level `await`s, and a component with one needs a `Suspense` ancestor.
- In Vue 3, `v-if` has higher priority than `v-for` on the same element, so the `v-if` cannot read the loop variable.

## By lens

### For memory-runtime

- A large payload that the view only reads — an API response, a parsed document, a long list of rows — stored in `ref()` or `reactive()` is deep-proxied. `shallowRef` replaced wholesale, or `markRaw` on the payload, keeps it out of the reactivity system. Component definitions and third-party class instances held in reactive state need `markRaw` too.
- A watcher created outside any scope, or a listener on `window`, `document` or a socket added in a composable with no matching removal on dispose, keeps its callback and captures alive for the page's life and multiplies on each remount.
- A module-level `Map`, `Set` or array in a composable that gains an entry per component, topic or id and never loses one grows without bound across navigation.
- A `watch` with `deep: true` on a large object traverses the whole tree on every trigger; watching the specific getter that matters avoids it.
- A long list rendered with `v-for` mounts one component per row; past a few hundred mostly off-screen rows, virtualisation is the fix.
- Bundle size: a heavy panel, editor or chart imported eagerly where `defineAsyncComponent` or a dynamic `import()` would split it; whole-library imports where per-function imports exist.
- Tree-shaking: the compile-time flags in the Vite `define` (`__VUE_OPTIONS_API__`, `__VUE_PROD_DEVTOOLS__`, `__VUE_PROD_HYDRATION_MISMATCH_DETAILS__`) decide what framework code ships; an app that uses no Options API can set `__VUE_OPTIONS_API__` to `false`. Development plugins such as `vite-plugin-vue-devtools` must not reach the production bundle. A barrel module with side effects keeps unused exports. Confirm sizes with the build's chunk report.

### For algorithm

- A method call or inline expression in a template re-runs on every render of the component; a `computed` re-runs only when its tracked dependencies change. A filter, sort or group over a large array belongs in a `computed`.
- A `computed` that returns a fresh array or object on each evaluation always counts as changed, so everything that reads it re-runs whenever any of its inputs changes.
- A `watch` that writes another ref which another `watch` reads forms a chain that runs one flush per link and can loop; data derivable from other state belongs in a `computed`.
- `v-for` matches rows by `:key`. An index key on a list that reorders, filters or inserts reuses the wrong DOM and component state; a key generated in render remounts every row.
- A computed or render that searches the whole list for each row makes the list quadratic; build a `Map` once in a `computed`.
- Many fine-grained `ref` writes in a tight loop where one write of the final value would serve, and `triggerRef` called per item instead of once per batch.

### For correctness

- Destructuring a `reactive()` object, or spreading it, copies plain values and loses reactivity; `toRefs` or reading through the object keeps it.
- Props destructured from `defineProps` (3.5 and later) are reactive in the template and in the compiled `setup`, but passing one to `watch` or to a composable passes its current value; it needs a getter (`() => count`) or `toRef`.
- `watch(state.count, ...)` watches a number, not the property; the source must be a ref, a reactive object or a getter.
- A ref read in script without `.value`, or a ref nested inside an array or a `Map`, which the template does not unwrap.
- A watcher that starts async work needs `onCleanup` (or `onWatcherCleanup` in 3.5 and later) or a staleness check, or an earlier slower response overwrites a newer one.
- A prop mutated in the child, or `v-model` bound to a prop, instead of emitting an update.
- `v-if` beside `v-for` on one element, written as if the loop variable were in scope.
- Lifecycle hooks, `onScopeDispose` or `inject` called after an `await` inside a composable, which silently attach to nothing.
- A composable meant for both refs and plain values that reads its argument once instead of through `toValue` inside a getter or `watchEffect`, so it never sees updates.

### For idioms

- Follow the repo's state pattern. A codebase built on module-singleton composables or provide/inject does not need Pinia, and a new store library is advisory, not cleanup.
- Composables are named `use*` and return refs, not unwrapped values, so callers keep reactivity.
- Type-based `defineProps<...>()` and `defineEmits<...>()` where the codebase uses them; `useTemplateRef` in 3.5 and later rather than a same-named ref; `<script setup vapor>` on every SFC in a Vapor-mode app.
- Options API components, mixins or `this`-based code added to a Composition API codebase.
- `watch` plus a ref used where a `computed` states the same derivation.
