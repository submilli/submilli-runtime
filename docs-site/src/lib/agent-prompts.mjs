import { agentIconPaths } from '../assets/agent-logos/paths.mjs';
import { h, s } from '@expressive-code/core/hast';

// Links prefill a composer; the reader chooses a project and sends the prompt.
// Sources and limits: see src/assets/agent-logos/README.md.
export const agents = [
  { id: 'codex', name: 'Codex', base: 'codex://threads/new?prompt=', icon: 'codex.svg' },
  { id: 'claude', name: 'Claude Code', base: 'claude://code/new?q=', icon: 'claude-code.svg' },
  { id: 'cursor', name: 'Cursor', base: 'cursor://anysphere.cursor-deeplink/prompt?text=', icon: 'cursor.svg' },
];
const icon = id => s('svg', { viewBox: '0 0 24 24', width: 18, height: 18, fill: 'currentColor', fillRule: 'evenodd', 'aria-hidden': 'true' }, agentIconPaths[id].map(d => s('path', { d, clipRule: 'evenodd' })));

export function promptLink(agent, prompt) {
  const href = agent.base + encodeURIComponent(prompt);
  // Cursor rejects URLs over 10,000 characters; Claude truncates long prompts.
  // Keep the same conservative limit for every destination, with Copy available.
  return href.length <= 10000 ? href : undefined;
}

export function agentPromptAst(prompt, { collapsed = false } = {}) {
  const launchers = agents.map(agent => {
    const href = promptLink(agent, prompt);
    const label = href ? `Open in ${agent.name}` : `${agent.name}: use Copy prompt for this long prompt`;
    return h(href ? 'a' : 'span', {
      className: ['agent-prompt-launch'], href, 'aria-label': label,
      'aria-disabled': href ? undefined : 'true',
      title: href ? `${label}. Choose your project, review, then send.` : label,
    }, [icon(agent.id)]);
  });
  const text = h('pre', { className: ['agent-prompt-text'], tabindex: 0 }, [h('code', prompt)]);
  return h('div', { className: ['agent-prompt'], 'data-agent-prompt': '' }, [
    h('div', { className: ['agent-prompt-bar'] }, [
      h('span', { className: ['agent-prompt-label'] }, 'Run in'),
      ...launchers,
      h('button', { type: 'button', className: ['agent-prompt-copy'], 'data-copy-prompt': '', disabled: true }, [
        s('svg', { viewBox: '0 0 24 24', width: 16, height: 16, fill: 'none', stroke: 'currentColor', strokeWidth: 1.7, 'aria-hidden': 'true' }, [
          s('rect', { x: 8, y: 8, width: 12, height: 13, rx: 2 }),
          s('path', { d: 'M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3' }),
        ]), h('span', { 'data-copy-label': '' }, 'Copy prompt'),
      ]),
    ]),
    collapsed ? h('details', [h('summary', 'View prompt'), text]) : text,
    h('div', { className: ['agent-prompt-fallback'], hidden: true }, [
      h('label', ['Select and copy the prompt', h('textarea', { readonly: true, rows: 5, 'aria-label': 'Prompt to copy' }, prompt)]),
    ]),
    h('span', { className: ['agent-prompt-status'], role: 'status', 'aria-live': 'polite' }),
  ]);
}
