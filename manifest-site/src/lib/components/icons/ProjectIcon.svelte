<script lang="ts">
	interface Props {
		size?: number;
		class?: string;
	}

	let { size = 16, class: className = '' }: Props = $props();
</script>

<!-- Open book icon for project root -->
<svg
	width={size}
	height={size}
	viewBox="0 0 16 16"
	fill="none"
	xmlns="http://www.w3.org/2000/svg"
	class={className}
	aria-label="Project root"
>
	<path
		d="M8 3.5V13M8 3.5C7 2.5 5 2 3 2.5V12C5 11.5 7 12 8 13M8 3.5C9 2.5 11 2 13 2.5V12C11 11.5 9 12 8 13"
		stroke="currentColor"
		stroke-width="1.5"
		stroke-linecap="round"
		stroke-linejoin="round"
	/>
</svg>
