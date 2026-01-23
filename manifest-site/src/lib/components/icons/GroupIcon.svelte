<script lang="ts">
	interface Props {
		size?: number;
		class?: string;
	}

	let { size = 16, class: className = '' }: Props = $props();
</script>

<!-- Four dots icon -->
<svg
	width={size}
	height={size}
	viewBox="0 0 16 16"
	fill="none"
	xmlns="http://www.w3.org/2000/svg"
	class={className}
	aria-label="Feature set"
>
	<circle cx="5" cy="5" r="2" fill="var(--foreground-muted)" />
	<circle cx="11" cy="5" r="2" fill="var(--foreground-muted)" />
	<circle cx="5" cy="11" r="2" fill="var(--foreground-muted)" />
	<circle cx="11" cy="11" r="2" fill="var(--foreground-muted)" />
</svg>
