<script lang="ts">
  interface Props {
    size?: number;
    class?: string;
  }

  let { size = 16, class: className = '' }: Props = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  xmlns="http://www.w3.org/2000/svg"
  class={className}
  aria-label="Settings"
>
  <path
    d="M6.5 1H9.5L10 2.5L11.5 3L13 2L14.5 3.5L13.5 5L14 6.5L15.5 7V10L14 10.5L13.5 12L14.5 13.5L13 15L11.5 14L10 14.5L9.5 16H6.5L6 14.5L4.5 14L3 15L1.5 13.5L2.5 12L2 10.5L0.5 10V7L2 6.5L2.5 5L1.5 3.5L3 2L4.5 3L6 2.5L6.5 1Z"
    stroke="currentColor"
    stroke-width="1.2"
    stroke-linejoin="round"
  />
  <circle cx="8" cy="8.5" r="2" stroke="currentColor" stroke-width="1.2" />
</svg>
