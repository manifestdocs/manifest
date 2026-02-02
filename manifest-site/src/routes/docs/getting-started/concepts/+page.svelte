<script lang="ts">
  import { base } from '$app/paths';
  import { CodeBlock } from '$lib/components/docs';
  import { StateIcon } from '$lib/components/icons';
</script>

<h1>Concepts</h1>

<p>Understanding Manifest's data model.</p>

<h2>Overview</h2>

<p>
  Manifest tracks features over the long term as living documentation rather
  than ephemeral tasks. Features form a tree that mirrors your product's
  architecture, and AI agents use it to understand what to build and to record
  what they've done.
</p>

<h2>Features</h2>

<p>
  A feature describes what your system does. Most features are user-facing
  (Password Reset, OAuth Login, Webhooks), but non-product features work too
  (Quality Documentation, Code Coverage, Quick Rollback).
</p>

<p>
  AI coding agents are good at breaking down features into tasks and organizing
  their own sub-agents for implementation. You define the feature, they figure
  out the steps.
</p>

<table>
  <thead>
    <tr>
      <th>Features</th>
      <th>Tasks</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Password Reset</td>
      <td>Refactor Auth</td>
    </tr>
    <tr>
      <td>Code Coverage</td>
      <td>Code Review</td>
    </tr>
    <tr>
      <td>Quick Rollback</td>
      <td>Sprint 12 Planning</td>
    </tr>
  </tbody>
</table>

<p>Feature details can be anything, but we recommend:</p>

<ul>
  <li>
    <strong>User story:</strong> Clarifies who benefits and why, helping agents understand
    intent
  </li>
  <li>
    <strong>Acceptance criteria:</strong> Defines "done" for both humans reviewing
    and agents implementing
  </li>
  <li>
    <strong>Technical notes:</strong> Constraints and context that prevent agents
    from making wrong assumptions
  </li>
</ul>

<h2>Feature tree</h2>

<p>Features form a hierarchy mirroring your product's architecture:</p>

<CodeBlock
  copyable={false}
  code={`MyProject
├── ● Authentication
│   ├── ● Password Login
│   ├── ○ OAuth Integration
│   └── ◇ Two-Factor Auth
└── ◇ Webhooks`}
/>

<p>
  Leaf features can be implemented directly. Parent features with children are
  called <strong>feature sets</strong>.
</p>

<h2>Feature sets</h2>

<p>
  Feature sets are features that contain other features. They organize related
  functionality into logical groups:
</p>

<CodeBlock
  copyable={false}
  code={`◇ Authentication          ← Feature set (has children)
├── ◇ Password Login      ← Leaf feature
├── ◇ OAuth Integration   ← Leaf feature
└── ◇ Two-Factor Auth     ← Leaf feature`}
/>

<p>Use feature sets to:</p>

<ul>
  <li>
    Group related features (e.g., all auth features under "Authentication")
  </li>
  <li>Mirror your codebase architecture</li>
  <li>Create navigation hierarchy in the feature tree</li>
</ul>

<p>
  Feature sets can be nested. A feature set's state reflects its children—it's
  complete when all children are implemented.
</p>

<h2>Feature states</h2>

<table>
  <thead>
    <tr>
      <th>State</th>
      <th>CLI</th>
      <th>Web</th>
      <th>Meaning</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Proposed</td>
      <td>◇</td>
      <td><StateIcon state="proposed" size={16} /></td>
      <td>Idea in the backlog</td>
    </tr>
    <tr>
      <td>In progress</td>
      <td>○</td>
      <td><StateIcon state="in_progress" size={16} /></td>
      <td>Currently being worked on</td>
    </tr>
    <tr>
      <td>Implemented</td>
      <td>●</td>
      <td><StateIcon state="implemented" size={16} /></td>
      <td>Complete and documented</td>
    </tr>
    <tr>
      <td>Archived</td>
      <td>✗</td>
      <td><StateIcon state="archived" size={16} /></td>
      <td>Soft-deleted, can be restored</td>
    </tr>
  </tbody>
</table>

<h2>Archiving and deleting</h2>

<p>Manifest uses a two-step deletion flow to prevent accidents:</p>

<ol>
  <li>
    <strong>Archive:</strong> Soft-deletes the feature. Archived features are hidden
    from normal views but can be restored.
  </li>
  <li>
    <strong>Delete permanently:</strong> Removes the feature from the database entirely.
    This cannot be undone.
  </li>
</ol>

<p>
  To delete a feature, first archive it, then delete it. This gives you a chance
  to recover if you archive something by mistake.
</p>

<h2>Feature history</h2>

<p>
  Every feature has an append-only history log (like <code>git log</code> for your
  product). When an agent completes work, it records a summary and links to commits.
</p>

<h2>Versions</h2>

<p>
  Versions group features into release milestones. Manifest organizes versions
  into Now (current focus), Next (queued up), and Later (backlog).
</p>

<p>
  See <a href="{base}/docs/cli/versions">Product versions</a> for the full workflow.
</p>
