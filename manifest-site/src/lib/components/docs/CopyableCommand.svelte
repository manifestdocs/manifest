<script lang="ts">
  let { command }: { command: string } = $props();

  let copied = $state(false);

  async function copyCommand() {
    await navigator.clipboard.writeText(command);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }
</script>

<div class="command-wrapper">
  <code class="command">{command}</code>
  <button class="copy-button" onclick={copyCommand} aria-label="Copy command">
    {#if copied}
      <svg
        width="18"
        height="18"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <polyline points="20 6 9 17 4 12" />
      </svg>
    {:else}
      <svg
        width="18"
        height="18"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
        <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
      </svg>
    {/if}
  </button>
</div>

<style>
  .command-wrapper {
    display: inline-flex;
    align-items: stretch;
    border: 1px solid var(--border-muted);
    margin: 1rem 0;
  }

  .command {
    display: inline-block;
    padding: 0.625em 1.25em;
    font-family: 'IBM Plex Mono', ui-monospace, monospace;
    font-size: 0.875em;
    color: var(--foreground);
    background: rgba(255, 255, 255, 0.02);
    user-select: all;
    cursor: text;
    border: none;
  }

  .copy-button {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 0.75em;
    background: var(--background);
    border: none;
    border-left: 1px solid var(--border-muted);
    color: var(--foreground-muted);
    cursor: pointer;
    transition: color 0.15s ease;
  }

  .copy-button:hover {
    color: var(--foreground);
  }
</style>
