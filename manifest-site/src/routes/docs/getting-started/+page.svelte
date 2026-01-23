<script lang="ts">
    import { base } from "$app/paths";
    import {
        Admonition,
        CodeBlock,
        CopyableCommand,
    } from "$lib/components/docs";
</script>

<h1>Install</h1>

<p>Get Manifest running on your machine.</p>

<h2>1. Install via Homebrew</h2>

<CopyableCommand command="brew install rocket-tycoon/tap/manifest" />

<p>
    This installs the <code>manifest</code> binary which runs the MCP server for AI
    agents.
</p>

<h2>2. Configure your AI agent</h2>

<p>
    Add Manifest as an MCP server so your AI agent can access project context.
</p>

<h3>Claude Code</h3>

<p>Run in your terminal:</p>

<CopyableCommand command="claude mcp add manifest -- manifest mcp" />

<h3>Goose</h3>

<p>Run in your terminal:</p>

<CopyableCommand command="goose mcp add manifest -- manifest mcp" />

<h3>OpenAI Operator</h3>

<p>Run in your terminal:</p>

<CopyableCommand command="operator mcp add manifest -- manifest mcp" />

<h2>3. Verify installation</h2>

<p>Check that the CLI is installed:</p>

<CopyableCommand command="manifest --version" />

<p>
    Start the server and open the web interface at <a
        href="http://localhost:17010"
        target="_blank"
        rel="noopener">http://localhost:17010</a
    >:
</p>

<CopyableCommand command="manifest serve" />

<h2>4. Claude Code plugin (optional)</h2>

<p>
    If you're using Claude Code, you can install the Manifest plugin for slash
    commands and enhanced workflows. Run this command <strong
        >inside Claude Code</strong
    >:
</p>

<CopyableCommand
    command="/plugin install rocket-tycoon/claude-plugins/manifest"
/>

<Admonition type="note">
    {#snippet children()}
        <p>
            This is a Claude Code slash command, not a terminal command. Run it
            in a Claude Code session.
        </p>
    {/snippet}
</Admonition>

<h2>Next step</h2>

<p>Continue to <a href="{base}/docs/cli/initialize">Initialize a project</a>.</p>
