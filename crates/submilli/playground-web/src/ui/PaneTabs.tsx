import type { KeyboardEvent } from "react";

const PANES = [
  { name: "chat", label: "Conversation" },
  { name: "code", label: "Program" },
  { name: "log", label: "Execution" },
] as const;

export type PaneName = (typeof PANES)[number]["name"];

export function PaneTabs({ active, onChange }: { active: PaneName; onChange: (pane: PaneName) => void }) {
  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    event.stopPropagation(); // Pane navigation must not advance recorded playback.
    const direction = event.key === "ArrowRight" ? 1 : -1;
    const index = PANES.findIndex((pane) => pane.name === active);
    const next = PANES[(index + direction + PANES.length) % PANES.length].name;
    onChange(next);
    document.getElementById("pg-tab-" + next)?.focus();
  };
  return (
    <div role="tablist" aria-label="Workspace pane" className="pg-pane-tabs" onKeyDown={onKey}>
      {PANES.map(({ name, label }) => (
        <button
          key={name}
          id={"pg-tab-" + name}
          role="tab"
          aria-selected={active === name}
          aria-controls={"pg-panel-" + name}
          tabIndex={active === name ? 0 : -1}
          className="pg-btn"
          onClick={() => onChange(name)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
