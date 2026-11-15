import DOMPurify from 'dompurify';
import { marked } from 'marked';
import { safeGithubUrl } from './ui';

export const MAX_NOTES_LENGTH = 200_000;

/** Upstream markdown is data: strip active content, tracking images and direct navigation. */
export function sanitizeReleaseNotes(markdown: string, repositoryUrl: string): string {
  const parsed = marked.parse(markdown.slice(0, MAX_NOTES_LENGTH), { async: false, gfm: true });
  const clean = DOMPurify.sanitize(parsed, {
    ALLOWED_TAGS: ['p', 'br', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'ul', 'ol', 'li', 'blockquote', 'pre', 'code', 'strong', 'em', 'del', 'hr', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'a', 'details', 'summary'],
    ALLOWED_ATTR: ['href', 'title', 'colspan', 'rowspan'],
    ALLOW_DATA_ATTR: false,
    ALLOW_ARIA_ATTR: false,
  });
  const template = document.createElement('template');
  template.innerHTML = clean;
  const base = safeGithubUrl(repositoryUrl) ?? 'https://github.com/';
  for (const link of template.content.querySelectorAll('a')) {
    const href = link.getAttribute('href');
    const safe = href ? safeGithubUrl(href, base) : null;
    if (safe) {
      // A context-menu or middle click must not bypass the host's URL validation.
      link.setAttribute('href', '#');
      link.setAttribute('data-external-url', safe);
      link.setAttribute('title', `在系统浏览器打开：${safe}`);
    } else {
      link.removeAttribute('href');
      link.setAttribute('title', '此链接不在支持范围内，可到 GitHub 查看原文');
    }
  }
  return template.innerHTML;
}
