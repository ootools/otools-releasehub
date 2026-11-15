<script lang="ts">
  import { onMount, onDestroy, type Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  let { title, description = '', wide = false, onclose, children }: {
    title: string; description?: string; wide?: boolean; onclose: () => void; children: Snippet;
  } = $props();
  const id = $props.id();
  let dialog: HTMLDialogElement;
  let previous: HTMLElement | null = null;
  onMount(() => {
    previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    dialog.querySelector<HTMLElement>('[data-initial-focus]')?.focus();
  });
  onDestroy(() => { dialog?.close(); previous?.focus(); });
</script>

<dialog bind:this={dialog} class:wide aria-labelledby={`${id}-title`} aria-describedby={description ? `${id}-description` : undefined} oncancel={(event) => { event.preventDefault(); onclose(); }}>
  <div class="modal-header">
    <div><h2 id={`${id}-title`}>{title}</h2>{#if description}<p id={`${id}-description`}>{description}</p>{/if}</div>
    <button class="icon-button" type="button" aria-label="关闭对话框" onclick={onclose}><Icon name="close" /></button>
  </div>
  {@render children()}
</dialog>
