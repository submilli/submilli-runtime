// Links prefill a composer; the reader chooses a project and sends the prompt.
// Sources and limits: see src/assets/agent-logos/README.md.
export const agents = [
  { id: 'codex', name: 'Codex', base: 'codex://threads/new?prompt=', icon: 'codex.svg' },
  { id: 'claude', name: 'Claude Code', base: 'claude://code/new?q=', icon: 'claude-code.svg' },
  { id: 'cursor', name: 'Cursor', base: 'cursor://anysphere.cursor-deeplink/prompt?text=', icon: 'cursor.svg' },
];
export function promptLink(agent, prompt) {
  const href = agent.base + encodeURIComponent(prompt);
  // Cursor rejects URLs over 10,000 characters; Claude truncates long prompts.
  // Keep the same conservative limit for every destination, with Copy available.
  return href.length <= 10000 ? href : undefined;
}

// Only these downloadable demo assets follow the page's hosting origin.
export function contextualizePrompt(prompt, pageUrl) {
  const page = new URL(pageUrl);
  if (page.protocol !== 'https:' && page.protocol !== 'http:') return prompt;
  return prompt.replace(
    /https:\/\/submilli\.ai(\/docs\/examples\/permission-demo\/basic-blueprint\.yaml)(?=$|[\s)\]>,;])/g,
    (_, path) => page.origin + path,
  );
}
