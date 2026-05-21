<script setup lang="ts">
// Hierarchy tree + detail panel. On mount it loads the work-item tree from the
// Pinia store and renders the root nodes via the recursive <TreeItem>. The
// store's `selectNode` action (invoked from inside TreeItem) sets `selectedId`
// and loads the selected node's detail, which this view renders in a panel
// beside the tree.
import { onMounted, computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useHierarchyStore } from '@/stores/hierarchy'
import TreeItem from '@/components/TreeItem.vue'

const store = useHierarchyStore()
const { tree, detail, selectedId, loading, error } = storeToRefs(store)

onMounted(() => {
  void store.loadTree()
})

/** The selected node's detail, or null when nothing is selected yet. */
const selected = computed(() => detail.value)

/**
 * Status options offered by the optional status dropdown. Status is free-text
 * server-side (slice 1), so this is a convenience list, not an enum contract;
 * the current value is always included so the <select> can reflect it.
 */
const STATUS_OPTIONS = ['open', 'in-progress', 'review', 'done', 'blocked']
const statusOptions = computed<string[]>(() => {
  const current = selected.value?.item.status
  if (current && !STATUS_OPTIONS.includes(current)) {
    return [current, ...STATUS_OPTIONS]
  }
  return STATUS_OPTIONS
})

function onStatusChange(event: Event): void {
  const id = selectedId.value
  if (!id) return
  const next = (event.target as HTMLSelectElement).value
  if (next && next !== selected.value?.item.status) {
    void store.changeStatus(id, next)
  }
}
</script>

<template>
  <main class="hierarchy">
    <section class="tree-panel">
      <h2>Hierarchy</h2>
      <p v-if="error" class="error">{{ error }}</p>
      <p v-else-if="loading && tree.length === 0" class="muted">Loading…</p>
      <p v-else-if="tree.length === 0" class="muted">No work items.</p>
      <ul v-else class="tree-root">
        <TreeItem v-for="root in tree" :key="root.id" :node="root" :depth="0" />
      </ul>
    </section>

    <section class="detail-panel">
      <p v-if="!selected" class="muted">Select a node to see its detail.</p>
      <template v-else>
        <header class="detail-head">
          <span class="kind">{{ selected.item.kind }}</span>
          <h2>{{ selected.item.title }}</h2>
        </header>

        <dl class="meta">
          <dt>Status</dt>
          <dd>
            <select :value="selected.item.status" @change="onStatusChange">
              <option v-for="opt in statusOptions" :key="opt" :value="opt">
                {{ opt }}
              </option>
            </select>
          </dd>
        </dl>

        <p v-if="selected.item.body" class="body">{{ selected.item.body }}</p>

        <section class="findings">
          <h3>Findings ({{ selected.findings.length }})</h3>
          <p v-if="selected.findings.length === 0" class="muted">None.</p>
          <ul v-else>
            <li v-for="f in selected.findings" :key="f.id" class="finding">
              <span class="badge" :data-severity="f.severity">{{ f.severity }}</span>
              <span class="category">{{ f.category }}</span>
              <span class="summary">{{ f.summary }}</span>
              <span class="fstatus">{{ f.status }}</span>
            </li>
          </ul>
        </section>

        <section class="context">
          <h3>Context blocks ({{ selected.context_blocks.length }})</h3>
          <p v-if="selected.context_blocks.length === 0" class="muted">None.</p>
          <ul v-else>
            <li v-for="cb in selected.context_blocks" :key="cb.id" class="ctx">
              <strong>{{ cb.title }}</strong>
              <p>{{ cb.body }}</p>
            </li>
          </ul>
        </section>
      </template>
    </section>
  </main>
</template>

<style scoped>
.hierarchy {
  display: grid;
  grid-template-columns: minmax(280px, 1fr) 2fr;
  gap: 1.5rem;
  align-items: start;
  padding: 1.5rem;
}

.tree-panel,
.detail-panel {
  min-width: 0;
}

.tree-root {
  margin: 0;
  padding: 0;
}

.muted {
  opacity: 0.6;
}

.error {
  color: #c00;
}

.detail-head {
  display: flex;
  align-items: baseline;
  gap: 0.5rem;
}

.kind {
  font-size: 0.7rem;
  text-transform: uppercase;
  opacity: 0.6;
}

.meta {
  display: flex;
  gap: 0.5rem;
  align-items: center;
  margin: 0.5rem 0;
}

.meta dt {
  font-weight: bold;
}

.meta dd {
  margin: 0;
}

.body {
  white-space: pre-wrap;
}

.finding {
  display: flex;
  gap: 0.5rem;
  align-items: baseline;
  list-style: none;
  padding: 0.25rem 0;
}

.badge {
  font-size: 0.7rem;
  text-transform: uppercase;
  border: 1px solid currentColor;
  border-radius: 3px;
  padding: 0 0.3rem;
}

.badge[data-severity='critical'] {
  color: #c00;
}

.badge[data-severity='warning'] {
  color: #c80;
}

.category {
  opacity: 0.7;
  font-size: 0.8rem;
}

.fstatus {
  margin-left: auto;
  opacity: 0.6;
  font-size: 0.8rem;
}

.context ul {
  padding-left: 1rem;
}

.ctx p {
  margin: 0.25rem 0 0.75rem;
  opacity: 0.85;
}
</style>
