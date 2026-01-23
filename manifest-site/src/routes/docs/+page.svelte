<script lang="ts">
    import { goto } from '$app/navigation';
    import { base } from '$app/paths';
    import { onMount } from 'svelte';

    onMount(() => {
        goto(`${base}/docs/getting-started`);
    });
</script>

<div class="redirect">
    <p>Redirecting to documentation...</p>
</div>

<style>
    .redirect {
        display: flex;
        align-items: center;
        justify-content: center;
        min-height: 50vh;
        color: var(--foreground-muted);
    }
</style>
