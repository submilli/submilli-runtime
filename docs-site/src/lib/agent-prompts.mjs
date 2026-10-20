import { agentIconPaths } from '../assets/agent-logos/paths.mjs';
import { h, s } from '@expressive-code/core/hast';

import { agents, promptLink } from './agent-prompt-context.mjs';
export { agents, promptLink } from './agent-prompt-context.mjs';

const icon = id => s('svg', { viewBox: '0 0 24 24', width: 18, height: 18, fill: 'currentColor', fillRule: 'evenodd', 'aria-hidden': 'true' }, agentIconPaths[id].map(d => s('path', { d, clipRule: 'evenodd' })));

function promptTextAst(prompt) {
  const blocks = prompt.split(/\n{2,}/);
  const children = [];
  for (const block of blocks) {
    const lines = block.split('\n');
    const listItems = lines.every(line => /^\s*(?:[-*]|\d+[.)])\s+/.test(line));
    if (listItems) {
      children.push(h('ul', { className: ['agent-prompt-list'] }, lines.map(line =>
        h('li', line.replace(/^\s*(?:[-*]|\d+[.)])\s+/, '')),
      )));
      continue;
    }
    children.push(h('p', lines.join(' ')));
  }
  return h('div', { className: ['agent-prompt-readable'] }, children);
}

export function agentPromptAst(prompt, { collapsed = true } = {}) {
  const launchers = agents.map(agent => {
    const href = promptLink(agent, prompt);
    const label = href ? `Open in ${agent.name}` : `${agent.name}: use Copy prompt for this long prompt`;
    return h(href ? 'a' : 'span', {
      className: ['agent-prompt-launch'], 'data-agent-launch': agent.id, href, 'aria-label': label,
      'aria-disabled': href ? undefined : 'true',
      title: href ? `${label}. Choose your project, review, then send.` : label,
    }, [icon(agent.id)]);
  });
  const text = h('div', { className: ['agent-prompt-text'], tabindex: 0 }, [promptTextAst(prompt)]);
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
