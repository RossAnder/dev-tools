<script setup vapor lang="ts">
// Recursive, zero-dependency tree node (per the plan's Research Note: a known
// depth-5 hierarchy needs no PrimeVue <Tree>). A <TreeItem> renders one
// WorkItemNode and recursively renders its children via <TreeItem> — in
// <script setup> a component may reference itself by its filename.
import { ref, computed } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import type { WorkItemNode } from '@/api'

const props = defineProps<{
  node: WorkItemNode
  /** Nesting depth, used only for indentation. Root nodes pass 0. */
  depth?: number
}>()

const { selectedId, selectNode } = useHierarchy()

/** Local expand/collapse state; nodes start expanded so the tree is visible. */
const expanded = ref(true)

const depth = computed(() => props.depth ?? 0)
const hasChildren = computed(() => props.node.children.length > 0)
const isSelected = computed(() => selectedId.value === props.node.id)

function toggle(): void {
  if (hasChildren.value) expanded.value = !expanded.value
}

function select(): void {
  void selectNode(props.node.id)
}
</script>

<template>
  <li class="tree-item">
    <div
      class="row"
      :class="{ selected: isSelected }"
      :style="{ paddingLeft: depth * 16 + 'px' }"
    >
      <button
        type="button"
        class="twisty"
        :class="{ hidden: !hasChildren }"
        :aria-label="expanded ? 'Collapse' : 'Expand'"
        @click="toggle"
      >
        {{ hasChildren ? (expanded ? '▾' : '▸') : '' }}
      </button>
      <button type="button" class="label" @click="select">
        <span class="kind">{{ node.kind }}</span>
        <span class="title">{{ node.title }}</span>
        <span class="status">{{ node.status }}</span>
      </button>
    </div>

    <ul v-if="hasChildren && expanded" class="children">
      <TreeItem
        v-for="child in node.children"
        :key="child.id"
        :node="child"
        :depth="depth + 1"
      />
    </ul>
  </li>
</template>

<style scoped>
.tree-item {
  list-style: none;
}

.children {
  margin: 0;
  padding: 0;
}

.row {
  display: flex;
  align-items: center;
  gap: 0.25rem;
  border-radius: 4px;
}

.row.selected {
  background: var(--color-background-mute, #eee);
}

.twisty {
  width: 1.25rem;
  flex: 0 0 auto;
  background: none;
  border: none;
  cursor: pointer;
  color: inherit;
  padding: 0;
}

.twisty.hidden {
  visibility: hidden;
  cursor: default;
}

.label {
  display: flex;
  align-items: baseline;
  gap: 0.5rem;
  flex: 1 1 auto;
  background: none;
  border: none;
  cursor: pointer;
  color: inherit;
  text-align: left;
  padding: 0.2rem 0.4rem;
  font: inherit;
}

.label:hover {
  text-decoration: underline;
}

.kind {
  font-size: 0.7rem;
  text-transform: uppercase;
  opacity: 0.6;
}

.status {
  font-size: 0.75rem;
  opacity: 0.7;
  margin-left: auto;
}
</style>
