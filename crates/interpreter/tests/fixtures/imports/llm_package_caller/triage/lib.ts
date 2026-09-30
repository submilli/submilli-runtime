import llm from "submilli:llm";

/** Classifies a ticket on the caller's behalf. */
export function classify(model: string, ticket: string): string {
  const c = llm.call(model, ticket);
  return c.text as string;
}
