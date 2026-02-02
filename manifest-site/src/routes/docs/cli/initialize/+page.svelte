<script lang="ts">
  import { base } from '$app/paths';
  import { CodeBlock } from '$lib/components/docs';
</script>

<h1>Initialize a Project</h1>

<p>Set up your first project with Manifest.</p>

<h2>Initialize a project</h2>

<p>
  Manifest works with both new and existing projects. In your project directory,
  ask your agent:
</p>

<CodeBlock code="Initialize this project in Manifest" />

<p>The agent will:</p>

<ul>
  <li>Analyze your codebase structure (if files exist)</li>
  <li>Create a project with appropriate settings</li>
  <li>Associate the current directory</li>
</ul>

<p>
  For existing codebases, the agent uses the analysis to suggest initial
  features based on what's already built.
</p>

<h2>Multiple repositories</h2>

<p>
  For projects spanning multiple repositories (web/api, microservices), run
  Manifest from a root directory that contains them all. If you have a directory
  outside the current working directory, ask the agent to add it:
</p>

<CodeBlock code="Add the ../api directory to this project" />

<h2>Next step</h2>

<p>Continue to <a href="{base}/docs/cli/planning">Planning features</a>.</p>
