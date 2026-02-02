<script lang="ts">
  import { base } from '$app/paths';
</script>

<h1>Plan View</h1>

<p>Version matrix for release planning.</p>

<p>The Plan view shows a version matrix for release planning.</p>

<h2>Layout</h2>

<ul>
  <li><strong>Feature tree</strong> on the left</li>
  <li><strong>Version columns</strong> on the right (Now, Next, Later)</li>
</ul>

<h2>Assigning features to versions</h2>

<p>Drag features between columns to assign them to versions.</p>

<h2>Managing features</h2>

<p>Right-click any feature for context menu options:</p>

<p><strong>For normal features:</strong></p>

<ul>
  <li>
    <strong>Add Child Feature:</strong> Create a new feature (automatically assigned
    to Now version)
  </li>
  <li><strong>Archive:</strong> Soft-delete the feature</li>
</ul>

<p><strong>For archived features:</strong></p>

<ul>
  <li><strong>Restore:</strong> Return the feature to proposed state</li>
  <li><strong>Delete Permanently:</strong> Remove from database</li>
</ul>

<h2>Adding versions</h2>

<p>
  Click the <strong>+</strong> button after the last version column to create a new
  version. Manifest suggests the next version number based on semver.
</p>

<h2>Releasing a version</h2>

<p>
  When all features in the Now version are implemented, a release button
  appears. Clicking it closes out the version and shifts Now to the next
  unreleased version.
</p>

<p>
  See <a href="{base}/docs/cli/versions">Product versions</a> for the full workflow.
</p>
