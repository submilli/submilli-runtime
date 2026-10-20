// Links prefill a composer; the reader chooses a project and sends the prompt.
// Sources and limits: see src/assets/agent-logos/README.md.
export const agents = [
  { id: 'codex', name: 'Codex', base: 'codex://threads/new?prompt=' },
  { id: 'claude', name: 'Claude Code', base: 'claude://code/new?q=' },
  { id: 'cursor', name: 'Cursor', base: 'cursor://anysphere.cursor-deeplink/prompt?text=' },
];
export function promptLink(agent, prompt) {
  const href = agent.base + encodeURIComponent(prompt);
  // Cursor rejects URLs over 10,000 characters; Claude truncates long prompts.
  // Keep the same conservative limit for every destination, with Copy available.
  return href.length <= 10000 ? href : undefined;
}

// Keep copied and launched setup prompts on the page the reader is viewing.
export function promptForPage(prompt, canonicalUrl, pageUrl) {
  const page = new URL(pageUrl);
  if (page.protocol !== 'http:' && page.protocol !== 'https:') return prompt;
  const sourceUrl = page.origin + page.pathname;
  return prompt.replace(/https?:\/\/[^\s<>"`]+/g, url =>
    url === canonicalUrl ? sourceUrl : url,
  );
}
