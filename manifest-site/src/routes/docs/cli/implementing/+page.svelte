<script lang="ts">
    import { base } from '$app/paths';
    import { CodeBlock } from '$lib/components/docs';
    import { StateIcon } from '$lib/components/icons';
</script>

<h1>Implementing Features</h1>

<p>With your feature tree planned, agents can start implementing. Manifest tracks state and records history as work progresses.</p>

<h2>Feature lifecycle</h2>

<div class="lifecycle">
    <div class="lifecycle-step">
        <StateIcon state="proposed" size={18} />
        <span class="state-label">Proposed</span>
    </div>
    <div class="lifecycle-arrow">→</div>
    <div class="lifecycle-step">
        <StateIcon state="in_progress" size={18} />
        <span class="state-label">In Progress</span>
    </div>
    <div class="lifecycle-arrow">→</div>
    <div class="lifecycle-step">
        <StateIcon state="implemented" size={18} />
        <span class="state-label">Implemented</span>
    </div>
</div>

<h2>Finding work</h2>

<CodeBlock code="What should we work on next?" />

<p>The agent finds the highest-priority proposed feature from the Now version.</p>

<h2>Starting work</h2>

<CodeBlock code="Implement OAuth" />

<p>Manifest records:</p>

<ul>
    <li>State transitions to <strong>in_progress</strong> (○)</li>
    <li>Other agents see this feature is claimed</li>
</ul>

<h2>Completing work</h2>

<p>When the agent finishes, Manifest records:</p>

<ul>
    <li>State transitions to <strong>implemented</strong> (●)</li>
    <li>History entry with summary of what was done</li>
    <li>Links to commits from the session</li>
</ul>

<CodeBlock copyable={false} code={`Feature: OAuth Integration (●)

History:
  2024-03-20 - Implemented Google OAuth with token refresh
               - Added OAuth config and providers
               - Built token storage with encryption
               Commits: abc123, def456`} />

<h2>Slash commands</h2>

<p>Claude Code plugin provides shortcuts:</p>

<table>
    <thead>
        <tr>
            <th>Command</th>
            <th>Description</th>
        </tr>
    </thead>
    <tbody>
        <tr>
            <td><code>/manifest:next</code></td>
            <td>Show highest-priority feature ready for work</td>
        </tr>
        <tr>
            <td><code>/manifest:feature [query]</code></td>
            <td>Search for and display feature details</td>
        </tr>
        <tr>
            <td><code>/manifest:start [feature]</code></td>
            <td>Begin work on a feature (or next priority if omitted)</td>
        </tr>
        <tr>
            <td><code>/manifest:complete</code></td>
            <td>Complete current in-progress feature with summary</td>
        </tr>
    </tbody>
</table>

<h2>Next step</h2>

<p>Continue to <a href="{base}/docs/cli/versions">Product versions</a>.</p>

<style>
    .lifecycle {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        margin: 1.5rem 0;
    }

    .lifecycle-step {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.625rem 1rem;
        border: 1px solid var(--border-muted);
        background: rgba(255, 255, 255, 0.02);
    }

    .state-label {
        font-weight: 500;
        color: var(--foreground);
    }

    .lifecycle-arrow {
        color: var(--foreground-muted);
        font-size: 1.25rem;
    }
</style>
