import llm from "submilli:llm";

interface Label {
  label: string;
}

/** Classifies a ticket on the caller's behalf, as a checked `Label`. */
export function classifyTyped(model: string, ticket: string): string {
  const c = llm.call<Label>(model, ticket);
  return c.ok ? c.value.label : c.reason;
}
