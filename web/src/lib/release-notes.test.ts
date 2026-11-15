// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { MAX_NOTES_LENGTH, sanitizeReleaseNotes } from './release-notes';

const base = 'https://github.com/owner/repo/releases/tag/v1';
function documentFor(markdown: string) {
  const node = document.createElement('div');
  node.innerHTML = sanitizeReleaseNotes(markdown, base);
  return node;
}

describe('untrusted release notes', () => {
  it('renders useful markdown without active HTML or tracking images', () => {
    const node = documentFor('## 更新说明\n\n- 修复问题\n\n<script>alert(1)</script><img src="https://example.com/track" onerror="alert(1)"><iframe src="https://example.com"></iframe><svg onload="alert(1)"></svg>');
    expect(node.querySelector('h2')?.textContent).toBe('更新说明');
    expect(node.querySelector('li')?.textContent).toBe('修复问题');
    expect(node.querySelector('script, img, iframe, svg')).toBeNull();
    expect(node.innerHTML).not.toContain('onerror');
    expect(node.innerHTML).not.toContain('onload');
  });
  it('routes safe links through a data attribute, never direct browser navigation', () => {
    const node = documentFor('[发行说明](https://github.com/owner/repo/releases)');
    const link = node.querySelector('a');
    expect(link?.getAttribute('href')).toBe('#');
    expect(link?.getAttribute('data-external-url')).toBe('https://github.com/owner/repo/releases');
    expect(link?.hasAttribute('target')).toBe(false);
  });
  it('removes dangerous links, other hosts and attacker-supplied routing attributes', () => {
    const node = documentFor('<a href="javascript:alert(1)" data-external-url="https://evil.example">bad</a>\n\n[外部](https://example.com)\n\n<a href="https://github.com/owner/repo" onclick="alert(1)" target="_blank" id="app">safe</a>');
    const links = [...node.querySelectorAll('a')];
    expect(links[0].hasAttribute('href')).toBe(false);
    expect(links[0].hasAttribute('data-external-url')).toBe(false);
    expect(links[1].hasAttribute('href')).toBe(false);
    expect(links[2].hasAttribute('onclick')).toBe(false);
    expect(links[2].hasAttribute('id')).toBe(false);
    expect(links[2].hasAttribute('target')).toBe(false);
    expect(links[2].getAttribute('data-external-url')).toBe('https://github.com/owner/repo');
  });
  it('resolves GitHub-root relative links safely', () => {
    const node = documentFor('[变更](/owner/repo/pull/42)');
    expect(node.querySelector('a')?.getAttribute('data-external-url')).toBe('https://github.com/owner/repo/pull/42');
  });
  it('bounds very large notes before parsing', () => {
    const node = documentFor('a'.repeat(MAX_NOTES_LENGTH + 50));
    expect(node.textContent?.trim().length).toBe(MAX_NOTES_LENGTH);
  });
});
