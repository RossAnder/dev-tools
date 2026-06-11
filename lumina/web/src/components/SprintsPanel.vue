<script setup vapor lang="ts">
import { onMounted } from 'vue'
import { useSprints } from '@/composables/useSprints'
import SprintCard from '@/components/SprintCard.vue'

const { sprints, selectedSprintId, status, error, loadSprints, selectSprint } = useSprints()

onMounted(() => {
  loadSprints()
})
</script>

<template>
  <div class="flex flex-col gap-2">
    <p v-if="error" class="text-blocked font-mono text-[11px]">{{ error }}</p>
    <p
      v-if="status === 'loading' && sprints.length === 0"
      class="text-[var(--faint)] font-mono text-[11px] tracking-[0.16em]"
    >
      LOADING…
    </p>
    <p
      v-else-if="sprints.length === 0"
      class="text-[var(--ghost)] font-mono text-[11px] italic"
    >
      No sprints yet.
    </p>
    <div v-else class="flex flex-col gap-2 overflow-y-auto max-h-[60vh]">
      <SprintCard
        v-for="entry in sprints"
        :key="entry.sprint.id"
        :sprint="entry.sprint"
        :worktree="entry.worktree ?? null"
        :selected="entry.sprint.id === selectedSprintId"
        @select="selectSprint(entry.sprint.id)"
      />
    </div>
  </div>
</template>
