<script lang="ts">
  import { MAX_NOTES_LENGTH, sanitizeReleaseNotes } from '../lib/release-notes';
  let { body, repositoryUrl, onopen }: { body: string; repositoryUrl: string; onopen: (url: string) => void } = $props();
  const html = $derived(sanitizeReleaseNotes(body, repositoryUrl));

  function interceptLinks(node: HTMLElement) {
    const activate = (event: MouseEvent) => {
      const target = event.target instanceof Element ? event.target.closest('a') : null;
      if (!target || !node.contains(target)) return;
      event.preventDefault();
      const url = target.getAttribute('data-external-url');
      if (url && (event.type === 'click' || event.button === 1)) onopen(url);
    };
    node.addEventListener('click', activate);
    node.addEventListener('auxclick', activate);
    return { destroy() { node.removeEventListener('click', activate); node.removeEventListener('auxclick', activate); } };
  }
</script>

{#if body.trim()}
  <div class="release-markdown" use:interceptLinks>{@html html}</div>
  {#if body.length > MAX_NOTES_LENGTH}<p class="field-hint">说明较长，已截取展示。可在 GitHub 查看完整内容。</p>{/if}
{:else}
  <p class="muted empty-notes">作者没有填写本次更新说明。你仍可以到 GitHub 查看发行记录。</p>
{/if}
