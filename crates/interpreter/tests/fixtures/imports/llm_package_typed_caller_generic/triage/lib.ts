import llm from "submilli:llm";

/** Classifies a ticket on the caller's behalf, as whatever type the caller names. */
export function classify<T>(model: string, ticket: string): llm.Completion<T> {
  return llm.call<T>(model, ticket);
}

/** The same through `batch`, and through a second generic layer. */
export function classifyAll<T>(model: string, tickets: string[]): llm.Completion<T>[] {
  return each<T>(model, tickets);
}

function each<T>(model: string, tickets: string[]): llm.Completion<T>[] {
  return llm.batch<T>(model, tickets);
}
