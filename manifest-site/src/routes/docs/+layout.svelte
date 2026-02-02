<script lang="ts">
  import { DocsSidebar } from '$lib/components/docs';
  import { browser } from '$app/environment';
  import { beforeNavigate } from '$app/navigation';
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();

  let mobileMenuOpen = $state(false);

  // Track where user came from (home vs app)
  const STORAGE_KEY = 'docs_referrer';

  let cameFromApp = $state(false);

  // On mount, check referrer and store it
  $effect(() => {
    if (!browser) return;

    // Check if we already have a stored referrer
    const stored = sessionStorage.getItem(STORAGE_KEY);
    if (stored) {
      cameFromApp = stored === 'app';
      return;
    }

    // Check document.referrer for initial navigation
    const referrer = document.referrer;
    if (referrer) {
      try {
        const url = new URL(referrer);
        // If referrer is from same origin and is an app route (not home or docs)
        if (url.origin === window.location.origin) {
          const isAppRoute =
            url.pathname.startsWith('/projects') ||
            url.pathname.startsWith('/app');
          sessionStorage.setItem(STORAGE_KEY, isAppRoute ? 'app' : 'home');
          cameFromApp = isAppRoute;
        }
      } catch {
        // Invalid URL, ignore
      }
    }
  });

  // Clear stored referrer when navigating away from docs
  beforeNavigate(({ to }) => {
    if (browser && to?.url && !to.url.pathname.startsWith('/docs')) {
      sessionStorage.removeItem(STORAGE_KEY);
    }
  });

  function toggleMenu() {
    mobileMenuOpen = !mobileMenuOpen;
  }

  function closeMenu() {
    mobileMenuOpen = false;
  }
</script>

<div class="docs-layout">
  <!-- Mobile header -->
  <header class="mobile-header">
    <button class="menu-button" onclick={toggleMenu} aria-label="Toggle menu">
      {#if mobileMenuOpen}
        <svg
          width="24"
          height="24"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M18 6 6 18" />
          <path d="m6 6 12 12" />
        </svg>
      {:else}
        <svg
          width="24"
          height="24"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <line x1="4" x2="20" y1="12" y2="12" />
          <line x1="4" x2="20" y1="6" y2="6" />
          <line x1="4" x2="20" y1="18" y2="18" />
        </svg>
      {/if}
    </button>
    <span class="mobile-title">Documentation</span>
  </header>

  <!-- Backdrop for mobile -->
  {#if mobileMenuOpen}
    <button class="backdrop" onclick={closeMenu} aria-label="Close menu"
    ></button>
  {/if}

  <!-- Mobile sidebar overlay -->
  <aside class="sidebar-mobile" class:open={mobileMenuOpen}>
    <DocsSidebar onNavigate={closeMenu} {cameFromApp} />
  </aside>

  <!-- Centered container for sidebar + content -->
  <div class="docs-container">
    <aside class="sidebar-desktop">
      <DocsSidebar {cameFromApp} />
    </aside>
    <main class="docs-main">
      <div class="docs-content">
        {@render children()}
      </div>
    </main>
  </div>
</div>

<style>
  .docs-layout {
    min-height: 100vh;
    background: var(--background);
    font-size: 110%;
  }

  /* Mobile header */
  .mobile-header {
    display: none;
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: 3.5rem;
    padding: 0 1rem;
    background: var(--background);
    border-bottom: 1px solid var(--border-muted);
    align-items: center;
    gap: 0.75rem;
    z-index: 100;
  }

  .menu-button {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.25rem;
    background: none;
    border: none;
    color: var(--foreground-muted);
    cursor: pointer;
  }

  .menu-button:hover {
    color: var(--foreground);
  }

  .mobile-title {
    font-weight: 600;
    color: var(--foreground);
  }

  /* Backdrop for mobile */
  .backdrop {
    display: none;
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    z-index: 40;
    border: none;
    cursor: pointer;
  }

  /* Mobile sidebar */
  .sidebar-mobile {
    display: none;
    position: fixed;
    top: 0;
    left: 0;
    bottom: 0;
    width: 16rem;
    z-index: 50;
    transform: translateX(-100%);
    transition: transform 0.2s ease;
    font-size: 110%;
  }

  .sidebar-mobile.open {
    transform: translateX(0);
  }

  /* Container positioned so sidebar border is at 1/4 width */
  .docs-container {
    display: flex;
    margin-left: calc(25vw - 16rem);
  }

  /* Desktop sidebar */
  .sidebar-desktop {
    flex-shrink: 0;
    width: 16rem;
    position: sticky;
    top: 0;
    height: 100vh;
    font-size: 110%;
  }

  /* Main content */
  .docs-main {
    flex: 1;
    min-width: 0;
  }

  .docs-content {
    max-width: 50rem;
    padding: 4.25rem 1.5rem 4rem 5.5rem;
  }

  /* Typography for docs pages */
  .docs-content :global(h1) {
    font-size: 2rem;
    font-weight: 700;
    margin: 0 0 1.5rem;
    color: var(--foreground);
  }

  .docs-content :global(h2) {
    font-size: 1.375rem;
    font-weight: 600;
    margin: 2rem 0 0.75rem;
    color: var(--foreground);
  }

  .docs-content :global(h3) {
    font-size: 1.125rem;
    font-weight: 600;
    margin: 1.5rem 0 0.5rem;
    color: var(--foreground);
  }

  .docs-content :global(h2:first-child),
  .docs-content :global(h3:first-child) {
    margin-top: 0;
  }

  .docs-content :global(p) {
    color: var(--foreground-muted);
    line-height: 1.7;
    margin: 0 0 1rem;
  }

  .docs-content :global(a) {
    color: var(--accent-green);
    text-decoration: none;
  }

  .docs-content :global(a:hover) {
    text-decoration: underline;
  }

  .docs-content :global(ul) {
    color: var(--foreground-muted);
    line-height: 1.4;
    padding-left: 2.5rem;
    margin: 0.5rem 0 1.5rem;
    list-style-type: disc;
  }

  .docs-content :global(ol) {
    color: var(--foreground-muted);
    line-height: 1.4;
    padding-left: 2.5rem;
    margin: 0.5rem 0 1.5rem;
    list-style-type: decimal;
  }

  .docs-content :global(li) {
    margin-bottom: 0.5rem;
  }

  /* Inline code only (not in components) */
  .docs-content :global(p > code),
  .docs-content :global(li > code),
  .docs-content :global(td > code) {
    padding: 0.125rem 0.375rem;
    font-size: 0.875em;
    font-family: 'IBM Plex Mono', ui-monospace, monospace;
    background: var(--background-muted);
    border: 1px solid var(--border-muted);
  }

  .docs-content :global(table) {
    width: 100%;
    margin: 1rem 0;
    border-collapse: collapse;
    font-size: 0.875rem;
  }

  .docs-content :global(th),
  .docs-content :global(td) {
    padding: 0.625rem 1rem;
    text-align: left;
    border: 1px solid var(--border-muted);
  }

  .docs-content :global(th) {
    font-weight: 600;
    background: var(--background-muted);
    color: var(--foreground);
  }

  .docs-content :global(td) {
    color: var(--foreground-muted);
  }

  .docs-content :global(strong) {
    color: var(--foreground);
    font-weight: 600;
  }

  /* Mobile styles */
  @media (max-width: 48rem) {
    .mobile-header {
      display: flex;
    }

    .sidebar-mobile {
      display: block;
    }

    .backdrop {
      display: block;
    }

    .sidebar-desktop {
      display: none;
    }

    .docs-main {
      padding-top: 3.5rem;
    }

    .docs-content {
      padding: 1.5rem 1rem 3rem;
    }

    .docs-content :global(h1) {
      font-size: 1.5rem;
    }
  }
</style>
