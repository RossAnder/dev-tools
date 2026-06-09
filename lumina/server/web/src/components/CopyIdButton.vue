<script setup vapor lang="ts">
import { ref, onBeforeUnmount } from 'vue'

const props = defineProps<{ id: string }>()

const copied = ref(false)
let timer: ReturnType<typeof setTimeout> | null = null

async function copy(event: Event): Promise<void> {
  event.stopPropagation()
  try {
    await navigator.clipboard.writeText(props.id)
    copied.value = true
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => {
      copied.value = false
    }, 1200)
  } catch {
    // clipboard unavailable (insecure context, denied permission) — silent no-op
  }
}

onBeforeUnmount(() => {
  if (timer) clearTimeout(timer)
})
</script>

<template>
  <span
    role="button"
    tabindex="0"
    :title="copied ? 'Copied' : 'Copy id'"
    :aria-label="copied ? 'Id copied to clipboard' : 'Copy id to clipboard'"
    @click="copy"
    @keydown.enter.prevent="copy"
    @keydown.space.prevent="copy"
    class="inline-flex items-center justify-center w-4 h-4 align-middle text-[var(--faint)] hover:text-[var(--accent)] focus:text-[var(--accent)] cursor-pointer transition-colors outline-none"
  >
    <span v-if="copied" class="text-[10px] leading-none">✓</span>
    <svg
      v-else
      width="11"
      height="11"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
    </svg>
  </span>
</template>
