<script lang="ts">
  import { base } from '$app/paths';
</script>

<h1>Web Interface</h1>

<p>Managing projects and features through the browser.</p>

<p>
  The web interface provides a higher-level view of your project, a way to step
  back from the code and see the full picture.
</p>

<p>
  When working with agents through the CLI, it's easy to get stuck in the weeds.
  The web interface gives you:
</p>

<ul>
  <li><strong>Bird's-eye view</strong> of your feature tree and progress</li>
  <li>
    <strong>Easier editing</strong> of feature specs without context switching
  </li>
  <li>
    <strong>Manual planning</strong> when you need to reorganize or reprioritize
  </li>
  <li><strong>Version management</strong> with drag-and-drop assignment</li>
</ul>

<p>
  The web interface is available at <a
    href="http://localhost:17010"
    target="_blank"
    rel="noopener">http://localhost:17010</a
  >. If it's not running, see <a href="{base}/docs/getting-started">Install</a>.
</p>

<h2>Navigation</h2>

<p>The header contains:</p>

<ul>
  <li>
    <strong>Project selector:</strong> Dropdown to switch between projects
  </li>
  <li>
    <strong><a href="{base}/docs/web/edit">Edit</a>:</strong> Feature tree and detail
    editing (default view)
  </li>
  <li>
    <strong><a href="{base}/docs/web/plan">Plan</a>:</strong> Version matrix for release
    planning
  </li>
  <li>
    <strong><a href="{base}/docs/web/history">History</a>:</strong> Timeline of implementation
    work
  </li>
</ul>
