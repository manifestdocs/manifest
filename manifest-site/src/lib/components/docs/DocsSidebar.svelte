<script lang="ts">
    import { page } from '$app/stores';

    const navigation = [
        {
            title: 'Getting Started',
            items: [
                { title: 'Install', href: '/docs/getting-started' },
                { title: 'Concepts', href: '/docs/getting-started/concepts' }
            ]
        },
        {
            title: 'CLI Workflow',
            items: [
                { title: 'Initialize', href: '/docs/cli/initialize' },
                { title: 'Planning', href: '/docs/cli/planning' },
                { title: 'Implementing', href: '/docs/cli/implementing' },
                { title: 'Versions', href: '/docs/cli/versions' }
            ]
        },
        {
            title: 'Web Interface',
            items: [
                { title: 'Overview', href: '/docs/web' },
                { title: 'Edit View', href: '/docs/web/edit' },
                { title: 'Plan View', href: '/docs/web/plan' },
                { title: 'History View', href: '/docs/web/history' }
            ]
        }
    ];

    let { onNavigate, cameFromApp = false }: { onNavigate?: () => void; cameFromApp?: boolean } = $props();

    const backLink = $derived(cameFromApp ? { href: '/projects', label: 'Back to App' } : { href: '/', label: 'Back to Home' });

    function isActive(href: string): boolean {
        return $page.url.pathname === href;
    }

    function handleClick() {
        onNavigate?.();
    }
</script>

<nav class="docs-sidebar">
    <a href={backLink.href} class="back-link">&larr; {backLink.label}</a>

    {#each navigation as section}
        <div class="nav-section">
            <h3 class="nav-section-title">{section.title}</h3>
            <ul class="nav-list">
                {#each section.items as item}
                    <li>
                        <a
                            href={item.href}
                            class="nav-link"
                            class:active={isActive(item.href)}
                            onclick={handleClick}
                        >
                            {item.title}
                        </a>
                    </li>
                {/each}
            </ul>
        </div>
    {/each}
</nav>

<style>
    .docs-sidebar {
        display: flex;
        flex-direction: column;
        gap: 1.5rem;
        padding: 1.5rem;
        width: 16rem;
        height: 100%;
        overflow-y: auto;
        border-right: 1px solid var(--border-muted);
        background: var(--background);
    }

    .back-link {
        color: var(--foreground-muted);
        text-decoration: none;
        font-size: 1.04rem;
        margin-bottom: 0.5rem;
    }

    .back-link:hover {
        color: var(--foreground);
    }

    .nav-section {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .nav-section-title {
        font-size: 0.89rem;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        color: var(--foreground);
        margin: 0;
    }

    .nav-list {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
    }

    .nav-link {
        display: block;
        padding: 0.375rem 0.75rem;
        font-size: 1.04rem;
        color: var(--foreground-muted);
        text-decoration: none;
        border-radius: 0.25rem;
        transition: background-color 0.15s ease, color 0.15s ease;
    }

    .nav-link:hover {
        background: var(--background-muted);
        color: var(--foreground);
    }

    .nav-link.active {
        background: var(--background-muted);
        color: var(--accent-green);
        font-weight: 500;
    }
</style>
