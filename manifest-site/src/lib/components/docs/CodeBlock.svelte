<script lang="ts">
  import hljs from 'highlight.js/lib/core';
  import json from 'highlight.js/lib/languages/json';
  import bash from 'highlight.js/lib/languages/bash';
  import toml from 'highlight.js/lib/languages/ini'; // ini works for toml

  hljs.registerLanguage('json', json);
  hljs.registerLanguage('bash', bash);
  hljs.registerLanguage('toml', toml);

  let {
    code,
    language = '',
    copyable = true,
  }: { code: string; language?: string; copyable?: boolean } = $props();

  let copied = $state(false);

  const highlighted = $derived.by(() => {
    if (language && hljs.getLanguage(language)) {
      return hljs.highlight(code, { language }).value;
    }
    return code;
  });

  async function copyCode() {
    await navigator.clipboard.writeText(code);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }
</script>

<div class="code-block">
  <pre><code class={language ? `language-${language}` : ''}
      >{@html highlighted}</code
    ></pre>
  {#if copyable}
    <button class="copy-button" onclick={copyCode} aria-label="Copy code">
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
  {/if}
</div>

<style>
  .code-block {
    display: flex;
    align-items: stretch;
    margin: 1rem 0;
    border: 1px solid var(--border-muted);
  }

  pre {
    flex: 1;
    margin: 0;
    padding: 0.625em 1.25em;
    overflow-x: auto;
    font-size: 0.875em;
    line-height: 1.5;
    background: rgba(255, 255, 255, 0.02);
  }

  code {
    font-family: 'IBM Plex Mono', ui-monospace, monospace;
  }

  /* Syntax highlighting - minimal dark theme */
  code :global(.hljs-attr),
  code :global(.hljs-section) {
    color: #7ee787;
  }

  code :global(.hljs-string) {
    color: #a5d6ff;
  }

  code :global(.hljs-number),
  code :global(.hljs-literal) {
    color: #79c0ff;
  }

  code :global(.hljs-keyword) {
    color: #ff7b72;
  }

  code :global(.hljs-comment) {
    color: #8b949e;
  }

  code :global(.hljs-punctuation) {
    color: #c9d1d9;
  }

  .copy-button {
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 0.625em 0.75em;
    background: transparent;
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
