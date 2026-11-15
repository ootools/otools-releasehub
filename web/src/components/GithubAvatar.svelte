<script lang="ts">
  import { githubAvatarUrl } from '../lib/github';
  let { url = null, name }: { url?: string | null; name: string } = $props();
  const safeUrl = $derived(githubAvatarUrl(url));
  let failedUrl = $state<string | null>(null);
</script>

<span class="github-avatar" aria-hidden="true">
  {#if safeUrl && failedUrl !== safeUrl}
    <img src={safeUrl} alt="" width="34" height="34" loading="lazy" referrerpolicy="no-referrer" onerror={() => failedUrl = safeUrl} />
  {:else}
    {name.replace(/[^A-Za-z\d\u4e00-\u9fff]/g, '').slice(0, 2).toUpperCase() || 'GH'}
  {/if}
</span>

<style>
  .github-avatar { width: 34px; height: 34px; flex: 0 0 34px; display: inline-flex; align-items: center; justify-content: center; border: 1px solid var(--border); border-radius: 6px; background: var(--surface-soft); color: var(--subtle); font-size: 12px; font-weight: 650; overflow: hidden; }
  img { width: 100%; height: 100%; object-fit: cover; }
</style>
