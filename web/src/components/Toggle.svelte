<script lang="ts">
  let { checked, label, disabled = false, onchange }: { checked: boolean; label: string; disabled?: boolean; onchange: () => void } = $props();
</script>
<button type="button" class="toggle" role="switch" aria-checked={checked} aria-label={label} {disabled} onclick={onchange}><span></span></button>
