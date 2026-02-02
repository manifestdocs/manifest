<script lang="ts">
  import type { Snippet } from 'svelte';

  type AdmonitionType = 'note' | 'tip' | 'warning' | 'skill';

  let {
    type = 'note',
    title,
    children,
  }: {
    type?: AdmonitionType;
    title?: string;
    children: Snippet;
  } = $props();

  const defaults: Record<AdmonitionType, { title: string; icon: string }> = {
    note: { title: 'Note', icon: 'info' },
    tip: { title: 'Tip', icon: 'lightbulb' },
    warning: { title: 'Warning', icon: 'alert' },
    skill: { title: 'Skill', icon: 'terminal' },
  };

  const displayTitle = $derived(title || defaults[type].title);
</script>

<div class="admonition admonition-{type}">
  <div class="admonition-title">
    {#if type === 'note'}
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <circle cx="12" cy="12" r="10" />
        <path d="M12 16v-4" />
        <path d="M12 8h.01" />
      </svg>
    {:else if type === 'tip'}
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <path
          d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5"
        />
        <path d="M9 18h6" />
        <path d="M10 22h4" />
      </svg>
    {:else if type === 'warning'}
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <path
          d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"
        />
        <path d="M12 9v4" />
        <path d="M12 17h.01" />
      </svg>
    {:else if type === 'skill'}
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <polyline points="4 17 10 11 4 5" />
        <line x1="12" y1="19" x2="20" y2="19" />
      </svg>
    {/if}
    <span>{displayTitle}</span>
  </div>
  <div class="admonition-content">
    {@render children()}
  </div>
</div>

<style>
  .admonition {
    margin: 1rem 0;
    border: 1px solid var(--border-muted);
  }

  .admonition-title {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.625rem 1rem;
    font-size: 0.875rem;
    font-weight: 600;
    border-bottom: 1px solid var(--border-muted);
  }

  .admonition-title span {
    color: var(--foreground);
  }

  .admonition-content {
    padding: 0.75rem 1rem;
    font-size: 0.875rem;
    color: var(--foreground-muted);
    line-height: 1.6;
  }

  .admonition-content :global(p) {
    margin: 0;
  }

  .admonition-content :global(code) {
    padding: 0.125rem 0.375rem;
    font-size: 0.85em;
    background: var(--background-emphasis);
    border: 1px solid var(--border-muted);
  }

  /* Note - blue */
  .admonition-note {
    border-left: 7px solid var(--accent-blue);
  }
  .admonition-note .admonition-title {
    color: var(--accent-blue);
  }

  /* Tip - green */
  .admonition-tip {
    border-left: 7px solid var(--accent-green);
  }
  .admonition-tip .admonition-title {
    color: var(--accent-green);
  }

  /* Warning - orange */
  .admonition-warning {
    border-left: 7px solid var(--accent-orange);
  }
  .admonition-warning .admonition-title {
    color: var(--accent-orange);
  }

  /* Skill - Claude orange */
  .admonition-skill {
    border-left: 7px solid #da7756;
  }
  .admonition-skill .admonition-title {
    color: #da7756;
  }
</style>
