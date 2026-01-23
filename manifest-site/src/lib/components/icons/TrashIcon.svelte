<script lang="ts">
	interface Props {
		size?: number;
		class?: string;
	}

	let { size = 14, class: className = '' }: Props = $props();
</script>

<svg
	width={size}
	height={size}
	viewBox="0 0 14 14"
	fill="none"
	xmlns="http://www.w3.org/2000/svg"
	class={className}
	aria-label="Delete"
>
	<path
		d="M2 3.5H12M5.5 3.5V2.5C5.5 2 6 1.5 6.5 1.5H7.5C8 1.5 8.5 2 8.5 2.5V3.5M3.5 3.5V11.5C3.5 12 4 12.5 4.5 12.5H9.5C10 12.5 10.5 12 10.5 11.5V3.5"
		stroke="currentColor"
		stroke-width="1.2"
		stroke-linecap="round"
		stroke-linejoin="round"
	/>
</svg>
