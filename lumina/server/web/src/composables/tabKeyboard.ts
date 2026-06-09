// Roving-tabindex keyboard reducer for the work-item detail lens TabStrip.
//
// Implements the FOCUS-move arithmetic of the WAI-ARIA Authoring Practices
// "Tabs" pattern under the MANUAL-activation model
// (https://www.w3.org/WAI/ARIA/apg/patterns/tabs/):
//
//   - ArrowRight / ArrowLeft move focus to the next / previous tab, wrapping
//     at the ends of the tablist.
//   - Home / End move focus to the first / last tab.
//   - Selecting (activating) a tab — and thereby revealing its panel — is a
//     SEPARATE concern triggered by Enter, Space, or click. In the manual
//     model, moving focus does NOT change the selected tab; only an explicit
//     activation does. This reducer therefore computes the next FOCUS index
//     ONLY; activation is the caller's (TabStrip's) responsibility.
//
// Pure: no DOM access, no side effects, no imports. Given the same arguments it
// always returns the same result, which keeps it trivially unit-testable.

/**
 * Compute the next roving-tabindex FOCUS index for a tablist.
 *
 * @param current the index of the currently-focused tab (0-based)
 * @param key the `KeyboardEvent.key` value of the keydown
 * @param count the number of tabs in the tablist
 * @returns the index focus should move to; `current` for any no-op key or when
 *   there is nothing to move to (`count <= 0`)
 */
export function nextTabIndex(current: number, key: string, count: number): number {
  if (count <= 0) return current

  switch (key) {
    case 'ArrowRight':
      return (current + 1) % count
    case 'ArrowLeft':
      return (current - 1 + count) % count
    case 'Home':
      return 0
    case 'End':
      return count - 1
    default:
      return current
  }
}
