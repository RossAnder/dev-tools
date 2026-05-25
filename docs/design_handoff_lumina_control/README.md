# Handoff: Lumina Control

A dense, single-screen "control surface" for an agentic planning + dispatch tool. Three columns: a left **hierarchy spine** (epic → feature → story → task), a center **focus lens** for the selected node, and a right **rail** with sprint composer + live agent stream. Aesthetic is warm noir / dark amber.

**Target stack for this handoff:** Vue 3.5 + Tailwind CSS v4.

---

## About the Design Files

The files in this bundle are **design references created as a React + vanilla-CSS HTML prototype**. They are **not production code** and should not be copied verbatim. Your task is to **recreate these designs inside the target Vue 3.5 + Tailwind v4 codebase**, using its existing component, routing, and styling patterns.

The React JSX should be read as **component structure + prop shape**, not as source. The vanilla `styles.css` should be read as **design tokens + visual spec** — most of it will be lifted into `tailwind.config` / `@theme` and per-component `<style scoped>` or utility classes.

### Source files included
- `Lumina Control.html` — entry point, loads scripts
- `app.jsx` — all React components (header, hierarchy spine, focus lens, sprint composer, agent list, footer, App)
- `tree-view.jsx` — alternate hierarchical tree visualization for the center column
- `styles.css` — full stylesheet with design tokens at the top
- `data.js` — fixture data (epics, features, stories, tasks, sprint, agents)
- `tweaks-panel.jsx` — runtime tweak panel (not part of the production design; ignore for porting)

---

## Fidelity

**High-fidelity (hifi).** Final colors, typography, spacing, and layout are intentional. Recreate pixel-faithfully. Interactions (drilling, drag-to-sprint, keyboard nav) are working in the prototype and should behave the same in production.

---

## Recommended Vue 3.5 / Tailwind 4 Setup

### 1. Tailwind v4 theme (`assets/css/main.css` or equivalent)

Tailwind v4 expects design tokens via the `@theme` directive. Translate the CSS custom properties from `styles.css` directly:

```css
@import "tailwindcss";

@theme {
  /* Surfaces */
  --color-bg: #0e0c08;
  --color-bg-deep: #08070550;
  --color-surface: #15120d;
  --color-surface-2: #1c1812;
  --color-surface-3: #251f17;
  --color-border: #2a2519;
  --color-border-strong: #3a3220;
  --color-border-faint: #1f1a13;

  /* Ink */
  --color-ink: #f0eadc;
  --color-ink-2: #cfc7b3;
  --color-muted: #8f8775;
  --color-faint: #5c5547;
  --color-ghost: #3a352b;

  /* Accent (amber/ochre, in OKLCH) */
  --color-accent: oklch(0.78 0.13 70);
  --color-accent-deep: oklch(0.62 0.13 65);
  --color-accent-glow: oklch(0.78 0.13 70 / 0.18);

  /* Status */
  --color-success: oklch(0.74 0.10 145);
  --color-in-flight: oklch(0.78 0.13 70);
  --color-queued: oklch(0.68 0.04 80);
  --color-blocked: oklch(0.65 0.16 28);
  --color-done: oklch(0.62 0.04 95);

  /* Typography */
  --font-sans: "Geist", "Inter Tight", ui-sans-serif, system-ui, sans-serif;
  --font-mono: "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  --font-display: "Instrument Serif", "Iowan Old Style", Georgia, serif;

  /* Radii */
  --radius-sm: 3px;
  --radius-DEFAULT: 5px;
  --radius-lg: 8px;
  --radius-xl: 14px;
}
```

This gives you utilities like `bg-surface`, `text-ink`, `border-border-strong`, `text-accent`, `font-mono`, `font-display`, `rounded-lg`, etc.

### 2. Google Fonts

Import in `index.html` or via `unhead`:
```
https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600&family=JetBrains+Mono:wght@400;500;600&family=Instrument+Serif:ital@0;1&display=swap
```

### 3. Component breakdown (Vue 3.5 SFCs)

Use `<script setup lang="ts">`. Suggested file structure:

```
src/
  features/lumina/
    LuminaControl.vue          # the App root — 3-column grid shell
    components/
      AppHeader.vue            # top bar: brand, command bar, sprint pill, agent pill, date
      AppFooter.vue            # status line at bottom: focus path, keyboard hints
      HierarchySpine.vue       # left rail: vertical breadcrumb of node + siblings
      SpineNode.vue            # individual spine item with diamond marker
      Breadcrumbs.vue          # path crumbs above the focus lens
      CenterToolbar.vue        # focus/tree toggle + actions
      FocusLensBody.vue        # routes between FocusLens and root portfolio view
      FocusLens.vue            # the hero card showing selected node
      LensStats.vue            # 4-column KPI strip inside the lens
      ChildGrid.vue            # grid of child nodes below the lens
      ChildCard.vue            # individual child node card
      TreeView.vue             # alternative laid-out node graph (port from tree-view.jsx)
      SprintComposer.vue       # right top: sprint header + drag-target queue
      AgentList.vue            # right bottom: live agent stream
      AgentCard.vue            # individual agent w/ progress, log lines, runtime
      StatusPill.vue           # status chip (in-flight, queued, blocked, done)
      ProgressLine.vue         # thin progress bar + % text
    composables/
      useLumina.ts             # state store (focusId, sprintQueue, agents) — Pinia recommended
      useAgentSimulator.ts     # the setInterval that bumps agent progress every 3s
      useKeyboardNav.ts        # Backspace = go up to parent
    data/
      fixtures.ts              # port of data.js — epics/features/stories/tasks/sprint/agents
    utils/
      hierarchy.ts             # getPathTo, getChildren, getSiblings
      sizeOf.ts                # T-shirt size mapping for story points (S/M/L/XL)
```

### 4. State management

Use **Pinia** for shared state:
- `focusId: string | null` — currently focused node
- `sprintQueue: string[]` — task IDs queued for dispatch
- `agents: Agent[]` — live-mutated by the simulator interval
- `view: 'focus' | 'tree'` — center column mode

Reactive computeds for `currentNode`, `currentPath`, `currentChildren`, `siblings`.

---

## Screens / Views

There is **one screen** with three column regions plus header/footer. Below is the full anatomy.

### Layout (root)

```
┌────────────────────────────────────────────────────────────────┐
│  HEADER  (56px, 1px bottom border)                             │
├────────┬─────────────────────────────────────┬─────────────────┤
│  LEFT  │  CENTER                             │  RIGHT          │
│  280px │  flex 1                             │  360px          │
│  spine │  breadcrumbs + lens + child grid    │  sprint+agents  │
│        │                                     │                 │
├────────┴─────────────────────────────────────┴─────────────────┤
│  FOOTER  (32px, 1px top border)                                │
└────────────────────────────────────────────────────────────────┘
```

CSS:
```css
.app {
  display: grid;
  grid-template-rows: 56px 1fr 32px;
  height: 100vh; width: 100vw;
}
.body {
  display: grid;
  grid-template-columns: 280px 1fr 360px;
  min-height: 0;
}
```

Responsive breakpoints:
- `≤1280px`: cols = `260px 1fr 320px`
- `≤1024px`: cols = `240px 1fr 300px`, hide center command bar
- `≤900px`: collapses to single column (current implementation hides spine/right; production may need a different strategy)

### Header

- Height **56px**, `border-bottom: 1px solid var(--border)`, `background: var(--bg)`, horizontal padding **20px**, internal **24px gap**.
- Grid `280px 1fr auto`.
- **Brand mark (left, 280px slot)**: 22×22 amber-bordered square containing rotated 45° `L` glyph in Instrument Serif italic 15px. Text "LUMINA · CONTROL" in mono 11px with letter-spacing 0.16em next to it.
- **Center**: a "command bar" — pill-shaped (`border-radius: 8px`, `border: 1px solid var(--border)`, `background: var(--surface)`, `padding: 6px 14px`). Contains a `›` chevron in `--faint`, an input placeholder `JUMP TO EPIC / FEATURE / STORY / TASK · OR /COMMAND`, and a `⌘K` kbd badge on the right.
- **Right**: three "pills":
  1. Live sprint indicator with a small pulsing dot (`--in-flight`), label `SPRINT SP-26`, then `committed/capacity ITEMS` in `--ink-2`.
  2. Agent count pill with `--accent` dot: `N AGENT(S) · LIVE`.
  3. Date pill (transparent border), `--faint`: `22 MAY 2026`.

Pill styling:
```css
.pill {
  display: inline-flex; align-items: center; gap: 8px;
  padding: 5px 10px;
  border: 1px solid var(--border-strong);
  background: var(--surface);
  border-radius: 999px;
  font-family: var(--font-mono); font-size: 10.5px;
  letter-spacing: 0.1em; text-transform: uppercase;
  color: var(--ink-2);
}
.dot { width: 6px; height: 6px; border-radius: 50%; background: var(--muted); }
.dot.live { background: var(--in-flight); animation: pulse 2s infinite; }
```

### Footer

- Height **32px**, `border-top: 1px solid var(--border)`, padding `0 20px`, mono 10.5px, `--muted`.
- Left: breadcrumb of the focused node's full path with `·` separators.
- Right: keyboard hints (`◀ FOCUS PARENT · BKSPC` etc.).

### LEFT — Hierarchy Spine

Vertical list of the current node + ancestors above + siblings interleaved. A vertical gradient line runs through the column to suggest continuity.

- Outer width **280px**, padding `28px 20px 28px 28px`, `border-right: 1px solid var(--border)`.
- Each `.spine-node`: `position: relative; padding: 10px 0 10px 28px;`
- **Diamond marker** at `left: 12px; top: 16px; width: 6px; height: 6px;` rotated 45°, 1px border `--border-strong` on `--bg`.
- **Focused node** marker: filled `--accent` with `box-shadow: 0 0 0 2px var(--bg), 0 0 0 3px var(--accent), 0 0 10px var(--accent-glow)` (a halo that reads as "you are here").
- **Ancestor** marker: hollow with `--accent-deep` border.
- **Sibling** rows are dimmed to `opacity: 0.55`, brighten to `0.95` on hover.
- Per row: a small mono type label (`EPIC`/`FEATURE`/`STORY`/`TASK`, 9.5px, letter-spacing 0.18em, `--faint`), then the node title in Geist 13.5px, then optional sub line (e.g. `S-001` ID or count) in mono 10px `--muted`.

The vertical spine line itself is a 1px column with a gradient that fades in/out at top/bottom:
```css
.spine::before {
  content: ""; position: absolute; left: 14px; top: 0; bottom: 0; width: 1px;
  background: linear-gradient(180deg, transparent, var(--border-strong) 8%, var(--border-strong) 92%, transparent);
}
```

### CENTER — Focus Lens

Breadcrumbs above, then the **lens card** (the hero), then **child grid** below.

#### Breadcrumbs
Inline chain of node titles separated by `›`, mono 10.5px, letter-spacing 0.12em, `--muted`. Clickable, jumps focus.

#### Lens card (`.lens`)
- `border: 1px solid var(--border)`, `padding: 32px 36px 36px`, `position: relative`, `margin-bottom: 36px`.
- Background: vertical gradient `linear-gradient(180deg, oklch(0.16 0.02 70) 0%, var(--bg) 100%)`.
- **Corner brackets** (decorative): `::before` at top-left, `::after` at bottom-right — 16×16 squares with two amber borders meeting at the corner. Use Tailwind pseudo-utilities or a small scoped style block.
- **Watermark** (item code in giant faint Instrument Serif italic, behind the text): HIDDEN by default. Available as an opt-in tweak in the prototype — likely skip for production unless explicitly desired.

##### Lens header (`.lens-head`)
Flex row, space-between. Left side ~flex 1; right side `min-width: 200px`, `align-items: flex-end`, gap 12.

Left side:
- **Type label** (`.lens-type`): mono 10.5px, letter-spacing 0.18em, `--faint`, uppercase. Format: `STORY · S-001`.
- **Title** (`.lens-title`): Instrument Serif 46px, line-height 1.05, letter-spacing -0.01em, `--ink`, `text-wrap: balance`. For `task` type, swap to Geist 36px weight 500 (sans-serif).
- **Summary** (`.lens-summary`): Geist 14.5px, `--ink-2`, line-height 1.55, max-width ~640px, top margin 14px.

Right side (when applicable):
- `StatusPill` (see below)
- `ProgressLine` (only on non-task nodes with a `progress` number)
- Optional `SIZE · M` mono 11px letter-spaced
- Optional `OWNER · name` mono 11px letter-spaced

##### Lens stats (`.lens-stats`)
4-column grid below the head, `border-top: 1px solid var(--border)`, `padding-top: 18px`, `margin-top: 28px`.

Each stat:
- `border-right: 1px solid var(--border-faint)`, `padding-right: 16px`, `padding-left: 18px` (skip padding-left on first child, border-right on last).
- `.k`: mono 10px, letter-spacing 0.18em, `--faint`, uppercase, `margin-bottom: 8px`. Examples: `FEATURES`, `IN FLIGHT`, `DONE`, `SIZE`.
- `.v`: **all stats use the mono variant** — `font-family: var(--font-mono)`, `font-size: 16px`, `font-weight: 500`, `--ink`. (Earlier the first column used a 26px display font; that was unified to mono per design review.)
- `.sub`: mono 10px, letter-spacing 0.1em, `--muted`, `margin-top: 6px`.

##### Task-specific lens additions
When the focused node is a `task`:
- **Action row** (`.actions`): 4 buttons in a flex row — `DISPATCH AGENT` (primary, filled `--accent`, dark text), `+ ADD TO SPRINT`, `EDIT`, `BLOCK`. Each: padding 9px 16px, mono 10.5px letter-spaced 0.14em uppercase, border 1px `--border-strong`, `background: var(--surface)`. Primary swaps border + bg to `--accent` and color to `--bg`.
- **Acceptance criteria** (`.acceptance`): h3 in mono 10.5px `--faint` uppercase showing `X / Y`, then `<ul>` of items with a 12×12 checkbox-style `.cbx`. Done items: filled `--accent` cbx, line-through ink-2 text.
- **Context** (`.context`): 2-column grid of cards (`Files`, `Related nodes`), each `border: 1px solid var(--border)`, `padding: 16px`, monospace listing.

#### Child grid (`.children-head` + `.children-grid`)
Below the lens. Header has the child type label in Geist 18px + count badge, plus filter tabs (`ALL`, `IN FLIGHT`, `QUEUED`, etc.) in mono 10.5px uppercase, with the active one bold/`--ink` and others `--muted`.

Each child card (`.child`):
- `border: 1px solid var(--border)`, `background: var(--surface)`, `padding: 18px 20px`, `border-radius: var(--r)`.
- Hover: border lifts to `--border-strong`, background `--surface-2`.
- Header row: mono type+ID (`STORY S-002`), right side `Status` pill.
- Title (Geist 15.5px weight 500), summary (13px `--ink-2` line-clamp 2).
- Footer row mono 10.5px `--muted`: `SIZE letter` + child count (`5 TASKS`) + `OWNER name`.

### RIGHT — Sprint + Agents

Two stacked cards in a 360px column. `border-left: 1px solid var(--border)`. Padding 0 (cards have their own).

#### Sprint Composer (`.sprint`)
- Top header: sprint ID + name in Geist 14.5px, range + count in mono 10.5px. Buttons `DISPATCH` (primary) and a kebab on the right.
- Drag target zone (`.queue`): dashed `border: 1px dashed var(--border-strong)`, `border-radius: 8px`, `padding: 12px`, min-height 120px. On drag-over, border becomes `--accent` and background `--accent-glow`.
- Empty state: centered mono "DRAG · TASK · HERE" in `--faint`.
- Queue items: small horizontal cards with ID, title, size letter, remove `×` button.
- Capacity meter at the bottom: thin progress bar showing `queue.length / capacity ITEMS`. Capacity defaults to **6** (T-shirt-mode units, not points).

#### Agent list (`.agents`)
- Header: `LIVE AGENTS` (mono 11px letter-spaced) + count.
- Scrollable list.
- Each `.agent`: `border: 1px solid var(--border)`, `padding: 14px 16px`, `border-radius: 5px`, margin-bottom 10.
- `.agent.active`: border becomes `--border-strong` (NOT `--accent-deep` — the earlier accent-deep treatment was too loud and was rolled back), background a subtle gradient `linear-gradient(180deg, var(--surface-2), var(--surface))`.
- `.agent.idle` dot: `--faint`. `.agent.blocked` dot: `--blocked`.
- Header row: `■ NAME` mono 11px (the square is a 6×6 inline-block in `--accent` for active, etc.), runtime on the right as plain mono 9.5px `--muted` (e.g. `02:14:32` — **no hourglass emoji**, that was removed).
- Body: current task title in Geist 12px `--ink-2`. Below it, a small mono **log stream** with timestamp + tag + line. Tag colored `--accent`, `--success` for OK lines, `--blocked` for errors. Lines clipped with `text-overflow: ellipsis`.
- Bottom: 1px progress bar at `--accent` with `box-shadow: 0 0 6px var(--accent-glow)`. Indeterminate variant animates a 30%-wide bar across.

---

## Interactions & Behavior

- **Click any spine node, breadcrumb, or child card** → sets `focusId` to that node's id. Center lens re-renders.
- **Backspace** (when not in an input) → moves focus to the current node's parent.
- **Toolbar toggle** in the center column flips between `focus` mode (the lens) and `tree` mode (the alternate visual graph in `tree-view.jsx`).
- **Drag a task** from the lens's child grid or from a story's task list **onto the sprint queue** → adds to `sprintQueue`. A short toast (`+ T-XYZ · ADDED TO SPRINT`) flashes at bottom-center for ~1.4s. Visually, the queue's dashed border highlights amber during drag-over.
- **Remove from sprint** → click the `×` on a queue item.
- **Dispatch** → flashes `▸ DISPATCHED N TASK(S) TO HARNESS`. In production this would hit an API.
- **Live agents** auto-tick: every 3 seconds, active agents' `progress` advances by a randomized small delta (0.002–0.014), and their `runtime` increments by ~3 seconds. Implement as `setInterval` in an `onMounted` hook; clean up in `onBeforeUnmount`.

### Animations / transitions
- Spine markers, child card hovers, pills: `transition: all 150ms ease`.
- Live dot pulse: `@keyframes pulse { 0%, 100% { opacity: 1 } 50% { opacity: 0.4 } }`, 2s infinite.
- Indeterminate progress: `@keyframes indet` translates `-30%` → `130%` over 1.4s linear infinite.

### Hover states
- Pills, buttons, child cards lighten background by one surface step (`--surface` → `--surface-2`) and strengthen their border (`--border` → `--border-strong`).
- Sibling spine nodes brighten from 0.55 → 0.95 opacity.

---

## State Management

```ts
interface Node {
  id: string;
  type: 'epic' | 'feature' | 'story' | 'task';
  parent: string | null;
  title: string;
  summary: string;
  status: 'in-flight' | 'queued' | 'blocked' | 'done' | 'draft';
  progress?: number;       // 0..1
  points?: number;         // 1..13 — displayed as S/M/L/XL
  owner?: string;
  children?: string[];
  acceptance?: { t: string; done: boolean }[];  // tasks only
  context?: { files?: string[]; related?: string[] };
  assignedAgent?: string | null;
  kpi?: { k: string; v: string | number; sub?: string; mono?: boolean }[];  // optional override
}

interface Sprint {
  id: string;          // 'SP-26'
  name: string;
  range: string;       // 'MAY 22 → JUN 05'
  capacity: number;    // ITEMS, not points (default 6)
  committed: number;
  queue: string[];     // task ids
}

interface Agent {
  id: string;
  name: string;        // 'harness-α'
  status: 'active' | 'idle' | 'blocked';
  currentTask?: string;
  progress: number;    // 0..1
  runtime: string;     // 'HH:MM:SS'
  log: { ts: string; tag: string; line: string; level?: 'info' | 'ok' | 'err' }[];
}

// Pinia store:
const useLumina = defineStore('lumina', () => {
  const focusId = ref<string | null>('S-001');
  const view = ref<'focus' | 'tree'>('focus');
  const sprintQueue = ref<string[]>([...sprintFixture.queue]);
  const agents = ref<Agent[]>(agentsFixture);
  const flash = ref<string | null>(null);

  const node = computed(() => focusId.value ? byId[focusId.value] : null);
  const path = computed(() => getPathTo(focusId.value));
  const activeAgentCount = computed(() => agents.value.filter(a => a.status === 'active').length);

  function setFocus(id: string | null) { focusId.value = id; }
  function addToSprint(id: string) { /* idempotent push + flash */ }
  function removeFromSprint(id: string) { /* filter */ }
  function dispatch() { /* flash + clear or send */ }
  return { focusId, view, sprintQueue, agents, flash, node, path, activeAgentCount, ... };
});
```

---

## Design Tokens (canonical reference)

### Colors
| Token | Value | Use |
| --- | --- | --- |
| `--bg` | `#0e0c08` | App background |
| `--bg-deep` | `#08070550` | Deepest sink (rarely used) |
| `--surface` | `#15120d` | Card surfaces |
| `--surface-2` | `#1c1812` | Hover surface |
| `--surface-3` | `#251f17` | Pressed / nested surface |
| `--border` | `#2a2519` | Standard 1px border |
| `--border-strong` | `#3a3220` | Active / hover border |
| `--border-faint` | `#1f1a13` | Internal dividers (stat columns) |
| `--ink` | `#f0eadc` | Primary text |
| `--ink-2` | `#cfc7b3` | Secondary text |
| `--muted` | `#8f8775` | Meta / labels |
| `--faint` | `#5c5547` | Tiny labels |
| `--ghost` | `#3a352b` | Almost-invisible separators |
| `--accent` | `oklch(0.78 0.13 70)` | Primary amber |
| `--accent-deep` | `oklch(0.62 0.13 65)` | Darker amber |
| `--accent-glow` | `oklch(0.78 0.13 70 / 0.18)` | Halo/glow |
| `--success` | `oklch(0.74 0.10 145)` | Done / OK |
| `--in-flight` | `oklch(0.78 0.13 70)` | In-flight (= accent) |
| `--queued` | `oklch(0.68 0.04 80)` | Queued |
| `--blocked` | `oklch(0.65 0.16 28)` | Blocked (red-orange) |
| `--done` | `oklch(0.62 0.04 95)` | Done (warm gray) |

### Type
| Family | Use |
| --- | --- |
| **Instrument Serif** (italic available) | Display titles, brand mark, large numerics. Sizes: 46/38/32 in lens titles, 220 in watermark |
| **Geist** (400/500/600) | Body text, UI copy, button labels (sans). |
| **JetBrains Mono** (400/500/600) | All metadata, type labels, IDs, kbd, runtime, log lines. Sizes: 9.5/10/10.5/11/16 |

Common type recipes:
- **Section / type label**: mono 10–10.5px, letter-spacing 0.16–0.18em, uppercase, `--faint`.
- **Stat key**: mono 10px, letter-spacing 0.18em, uppercase, `--faint`.
- **Stat value (mono variant)**: mono 16px, weight 500, `--ink`.
- **Lens title**: Instrument Serif 46px, line-height 1.05, letter-spacing -0.01em, `text-wrap: balance`.
- **Body summary**: Geist 14.5px, line-height 1.55, `--ink-2`.
- **Pill / kbd**: mono 10.5px, letter-spacing 0.1em, uppercase.

### Spacing scale (informal)
The prototype uses ad-hoc values: 4, 6, 8, 10, 12, 14, 16, 18, 20, 24, 28, 32, 36. Lift these into Tailwind's default scale (`p-1` 4px, `p-2` 8px, `p-3` 12px, `p-4` 16px, `p-5` 20px, `p-6` 24px, `p-7` 28px, `p-8` 32px, `p-9` 36px) — no extension needed.

### Radii
| Token | Value |
| --- | --- |
| `--r-sm` | 3px |
| `--r` | 5px |
| `--r-lg` | 8px |
| `--r-xl` | 14px |

Buttons + cards mostly use 5px; pills use 999px (fully round); the command bar uses 8px.

### Shadows / glows
The design avoids ambient drop shadows. Glows are color-tinted only:
- Focused spine marker: `box-shadow: 0 0 0 2px var(--bg), 0 0 0 3px var(--accent), 0 0 10px var(--accent-glow);`
- Progress bar: `box-shadow: 0 0 6px var(--accent-glow);`
- Background grain: two large radial gradients in body background (see `styles.css` top).

### Sizing (T-shirt mapping)
Story `points` numbers convert to letters for display:
```ts
function sizeOf(pts?: number): 'S' | 'M' | 'L' | 'XL' | '—' {
  if (!pts) return '—';
  if (pts <= 2) return 'S';
  if (pts <= 5) return 'M';
  if (pts <= 8) return 'L';
  return 'XL';
}
```
Aggregates show distribution like `2S · 3M · 1L`.

---

## Assets

- **Fonts**: Geist, JetBrains Mono, Instrument Serif — all from Google Fonts. No custom icon font or sprites.
- **Icons / glyphs**: All UI ornaments are pure Unicode/CSS — `›`, `◀`, `▸`, `·`, `◆`, `◉`, `⌘K`, `+`, `×`, the rotated-square diamond, the corner brackets. **No emoji**; the previous `⌛` next to agent runtime was removed.
- **Imagery**: none. The design is text + lines only.

---

## Files in this bundle

| File | Role |
| --- | --- |
| `Lumina Control.html` | Entry HTML — script loading order only |
| `app.jsx` | All React components in one file (~900 lines) |
| `tree-view.jsx` | Alternate tree visualization |
| `styles.css` | Full stylesheet (~1500 lines, tokens at top) |
| `data.js` | Fixture data — port to TypeScript fixtures |
| `tweaks-panel.jsx` | Runtime tweak panel — **ignore for production port** |
| `screenshots/` | Reference renders of each main state (see below) |

Open `Lumina Control.html` in a browser to interact with the live reference.

### Screenshots

| File | What it shows |
| --- | --- |
| `screenshots/01-story-focus.png` | Default state — focused on a STORY (`S-001`). Shows the spine drilled to story depth with siblings beneath, the lens hero with `IN FLIGHT` status + size, child task grid below (cut off here), and the sprint composer + live agent stream on the right. |
| `screenshots/02-feature-focus.png` | Focused on a FEATURE (`F-001`). Lens now shows a progress bar (60%) since features have completion %, and the spine collapses one level. "Saved Views" section appears in the left rail since the spine has room. |
| `screenshots/03-epic-focus.png` | Focused on an EPIC (`EP-001`). Top-level node with sibling epics visible in the spine, summary text wrapping naturally, and the "Saved Views" rail panel showing through. |
| `screenshots/04-task-focus.png` | Focused on a TASK (`T-001`). Lens title shrinks to 36px sans-serif (Geist) for tasks specifically, shows acceptance criteria list and context cards. Spine drills all the way down to TASK row with siblings listed. |

---

## Notes for the implementer

- The CSS variable system maps almost 1:1 to Tailwind v4's `@theme`. **Stay token-driven** — don't bake hex values into utility classes; prefer `text-ink` over `text-[#f0eadc]`.
- The lens **corner brackets** are the signature visual; preserve them. They're not optional ornament — they replace a heavier full border accent that was rejected during review.
- **Don't reintroduce**: a top-edge accent border on the lens card, an orange border on active agent cards, the hourglass emoji on agent runtimes, or the giant background watermark of item codes. All of these were removed during design review.
- **All stat values use the mono treatment** — there was an earlier display-font variant for the first column that was unified away. Keep them consistent.
- The hierarchy spine's vertical line should fade in/out at top/bottom via the gradient — sharp ends look amateur.
- T-shirt sizing replaces numeric points everywhere user-facing, but the underlying `points` numbers stay in the data model.
- Sprint capacity is in **items**, not points (default 6).
- The watermark and other tweaks in `tweaks-panel.jsx` are prototype-only knobs — don't carry them into production.
