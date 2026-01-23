<script lang="ts">
    import { base } from '$app/paths';
    import { Admonition, CodeBlock } from '$lib/components/docs';
</script>

<h1>Planning Features</h1>

<p>Break down a PRD or describe your initial feature set.</p>

<p>After initializing your project, define what it can do.</p>

<h2>From a PRD</h2>

<p>Paste or link to your PRD (product requirements document) and ask:</p>

<CodeBlock code="Break down this PRD into features" />

<p>The agent will identify features and create a feature tree.</p>

<h2>From a description</h2>

<p>Describe what you're building:</p>

<CodeBlock code={`This is an e-commerce app. Users can browse products,
add items to a cart, and checkout with Stripe.`} />

<p>The agent will identify features like:</p>

<ul>
    <li>Product Catalog</li>
    <li>Shopping Cart</li>
    <li>Stripe Checkout</li>
</ul>

<h2>Add individual features</h2>

<p>For a single feature:</p>

<CodeBlock code="Add a feature for OAuth login" />

<p>For related features:</p>

<CodeBlock code="Plan features for user authentication: password login, OAuth, and password reset" />

<Admonition type="skill" title="Claude Code slash command /manifest:plan">
    {#snippet children()}
        <p>Use <code>/manifest:plan</code> for an interactive planning session. The agent will guide you through feature decomposition and confirm before creating.</p>
    {/snippet}
</Admonition>

<h2>Review the tree</h2>

<p>Ask to see what you have:</p>

<CodeBlock code="Show me the feature tree" />

<p>The agent will display:</p>

<CodeBlock copyable={false} code={`MyProject
├── ◇ Authentication
│   ├── ◇ Password Login
│   ├── ◇ OAuth
│   └── ◇ Password Reset
├── ◇ Product Catalog
├── ◇ Shopping Cart
└── ◇ Stripe Checkout

Legend: ◇ proposed  ○ in_progress  ● implemented  ✗ deprecated`} />

<Admonition type="skill" title="Claude Code slash command /manifest:tree">
    {#snippet children()}
        <p>Use <code>/manifest:tree</code> to quickly display the feature hierarchy with status symbols.</p>
    {/snippet}
</Admonition>

<h2>Organize with hierarchy</h2>

<p>Group related features under parents:</p>

<CodeBlock code="Move OAuth under Authentication" />

<p>Or create a parent for existing features:</p>

<CodeBlock code="Group Product Catalog, Shopping Cart, and Checkout under a Storefront parent" />

<p>
    Parent features can include context that flows to children: architectural decisions, shared conventions,
    or technical constraints that apply to all features in that group.
</p>

<h2>Next step</h2>

<p>Continue to <a href="{base}/docs/cli/implementing">Implementing features</a>.</p>
