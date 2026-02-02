<script lang="ts">
  // Feature state type (matches API schema)
  type FeatureState = 'proposed' | 'in_progress' | 'implemented' | 'archived';

  interface Props {
    state: FeatureState;
    size?: number;
    class?: string;
  }

  let { state, size = 16, class: className = '' }: Props = $props();
</script>

{#if state === 'proposed'}
  <!-- Diamond (◇) -->
  <svg
    width={size}
    height={size}
    viewBox="0 0 16 16"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    class={className}
    aria-label="Proposed"
  >
    <path d="M8 2L14 8L8 14L2 8Z" fill="var(--state-proposed)" />
  </svg>
{:else if state === 'in_progress'}
  <!-- Ring (green) -->
  <svg
    width={size}
    height={size}
    viewBox="0 0 16 16"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    class={className}
    aria-label="In Progress"
  >
    <circle
      cx="8"
      cy="8"
      r="5.5"
      stroke="var(--state-in-progress)"
      stroke-width="1.5"
      fill="none"
    />
  </svg>
{:else if state === 'implemented'}
  <!-- Filled circle with checkmark -->
  <svg
    width={size}
    height={size}
    viewBox="0 0 16 16"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    class={className}
    aria-label="Implemented"
  >
    <circle cx="8" cy="8" r="7" fill="var(--state-implemented)" />
    <path
      d="M4.5 8L7 10.5L11.5 5.5"
      stroke="var(--background)"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  </svg>
{:else if state === 'archived'}
  <!-- Archive/box icon (gray) -->
  <svg
    width={size}
    height={size}
    viewBox="0 0 16 16"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    class={className}
    aria-label="Archived"
  >
    <path
      d="M6.5 8C6.22386 8 6 8.22386 6 8.5C6 8.77614 6.22386 9 6.5 9H9.5C9.77614 9 10 8.77614 10 8.5C10 8.22386 9.77614 8 9.5 8H6.5ZM1 3.5C1 2.67157 1.67157 2 2.5 2H13.5C14.3284 2 15 2.67157 15 3.5V4.5C15 5.15311 14.5826 5.70873 14 5.91465V11.5C14 12.8807 12.8807 14 11.5 14H4.5C3.11929 14 2 12.8807 2 11.5V5.91465C1.4174 5.70873 1 5.15311 1 4.5V3.5ZM2.5 3C2.22386 3 2 3.22386 2 3.5V4.5C2 4.77614 2.22386 5 2.5 5H13.5C13.7761 5 14 4.77614 14 4.5V3.5C14 3.22386 13.7761 3 13.5 3H2.5ZM3 6V11.5C3 12.3284 3.67157 13 4.5 13H11.5C12.3284 13 13 12.3284 13 11.5V6H3Z"
      fill="var(--state-archived)"
    />
  </svg>
{/if}
