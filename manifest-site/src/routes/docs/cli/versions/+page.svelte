<script lang="ts">
    import { base } from '$app/paths';
    import { Admonition, CodeBlock } from '$lib/components/docs';
</script>

<h1>Product Versions</h1>

<p>Plan releases and track progress with version milestones.</p>

<p>
    Versions let you group features into releases and track progress toward milestones.
    Manifest uses semantic versioning (0.1, 0.2, 1.0) and organizes work with a Now / Next / Later model.
</p>

<h2>Now / Next / Later</h2>

<p>Manifest automatically categorizes your versions into three planning horizons:</p>

<table>
    <thead>
        <tr>
            <th>Horizon</th>
            <th>Meaning</th>
        </tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Now</strong></td>
            <td>The first unreleased version (your current focus)</td>
        </tr>
        <tr>
            <td><strong>Next</strong></td>
            <td>The second unreleased version (what's queued up)</td>
        </tr>
        <tr>
            <td><strong>Later</strong></td>
            <td>All other unreleased versions (the backlog)</td>
        </tr>
    </tbody>
</table>

<p>You can ask an agent to show you features by upcoming version:</p>

<CodeBlock code="Show me what's in the next few versions" />

<CodeBlock copyable={false} code={`0.2 (now)
├── ● OAuth Integration
└── ○ Rate Limiting

0.3 (next)
└── ◇ Webhooks

Unversioned
└── ◇ GraphQL API`} />

<Admonition type="skill" title="Claude Code slash command /manifest:versions">
    {#snippet children()}
        <p>Use <code>/manifest:versions</code> to display the version roadmap with now/next/later status and feature counts.</p>
    {/snippet}
</Admonition>

<h2>Creating versions</h2>

<p><strong>You say:</strong></p>

<CodeBlock code="Create version 0.2 for the auth release" />

<p><strong>Agent handles:</strong></p>

<ul>
    <li>Creates the version milestone</li>
    <li>Version becomes available for feature assignment</li>
</ul>

<p>Use semantic versioning: <code>0.1</code>, <code>0.2</code>, <code>1.0</code>. You can also use names like <code>MVP</code> or <code>beta</code>.</p>

<h2>Assigning features</h2>

<p><strong>You say:</strong></p>

<CodeBlock code="Put OAuth in 0.2" />

<p><strong>Agent handles:</strong></p>

<ul>
    <li>Assigns the feature to the version</li>
    <li>Feature now contributes to version progress</li>
</ul>

<Admonition type="skill" title="Claude Code slash command /manifest:assign [feature] [version]">
    {#snippet children()}
        <p>Use <code>/manifest:assign OAuth 0.2</code> to schedule a feature for a specific release.</p>
    {/snippet}
</Admonition>

<h2>How versions affect work order</h2>

<p>Agents prioritize work based on versions:</p>

<ol>
    <li><strong>Now version first:</strong> Features in the current release get priority</li>
    <li><strong>Priority within version:</strong> Lower priority number = higher importance</li>
    <li><strong>Backlog last:</strong> Unassigned features after versioned work</li>
</ol>

<p>This means when you ask "What should we work on next?", the agent pulls from Now before anything else.</p>

<h2>Closing out a version</h2>

<p>When all features in a version are implemented, the agent automatically closes it:</p>

<p><strong>Agent handles:</strong></p>

<ul>
    <li>Detects all features in the version are <code>implemented</code> (●)</li>
    <li>Calls <code>release_version</code> to close it out</li>
    <li>Sets <code>released_at</code> timestamp</li>
    <li>Shifts "now" to the next unreleased version</li>
</ul>

<p>This keeps planning simple: finish what's in Now, then Now becomes the next version automatically.</p>

<p>You can also close a version manually:</p>

<CodeBlock code="Close out 0.2" />

<Admonition type="skill" title="Claude Code slash command /manifest:release [version]">
    {#snippet children()}
        <p>Use <code>/manifest:release 0.2</code> to mark a version as shipped, or <code>/manifest:release</code> to release the current "now" version.</p>
    {/snippet}
</Admonition>

<h2>Managing versions in the web interface</h2>

<p>The web interface provides a visual version matrix:</p>

<CodeBlock copyable={false} code={`┌──────────────────────────────────────────────────────────┐
│ Feature           │ 0.2 (now) │ 0.3 (next) │ Unversioned │
├──────────────────────────────────────────────────────────┤
│ OAuth Integration │     ●     │            │             │
│ Rate Limiting     │     ○     │            │             │
│ Webhooks          │           │     ◇      │             │
│ GraphQL API       │           │            │      ◇      │
└──────────────────────────────────────────────────────────┘`} />

<p>Drag features between columns to reassign versions.</p>

<h2>Next step</h2>

<p>Continue to <a href="{base}/docs/web">Web interface</a>.</p>
