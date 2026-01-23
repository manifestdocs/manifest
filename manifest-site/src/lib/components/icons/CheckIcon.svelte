<script lang="ts">
	interface Props {
		size?: number;
		class?: string;
	}

	let { size = 12, class: className = '' }: Props = $props();
</script>

<svg
	width={size}
	height={size}
	viewBox="0 0 12 12"
	fill="none"
	xmlns="http://www.w3.org/2000/svg"
	class={className}
	aria-label="Complete"
>
	<path
		d="M2 6L5 9L10 3"
		stroke="currentColor"
		stroke-width="2"
		stroke-linecap="round"
		stroke-linejoin="round"
	/>
</svg>
