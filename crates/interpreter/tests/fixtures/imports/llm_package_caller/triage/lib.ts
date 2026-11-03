import llm from "submilli:llm";

/** Classifies a ticket on the caller's behalf. */
export function classify(model: string, ticket: string): string {
  const c = llm.call(model, ticket);
  return c.ok ? c.value : "";
}

interface Label {
  label: string;
}

/** The typed form compiles inside a package too: its check closure is built
 *  in this module, not in the script that calls it. */
export function classifyTyped(model: string, ticket: string): string {
  const c = llm.call<Label>(model, ticket);
  return c.ok ? c.value.label : c.reason;
}
