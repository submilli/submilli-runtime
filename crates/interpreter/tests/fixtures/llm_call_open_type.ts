// A generic may forward its own `T` into `llm.call<T>`: the caller names the
// type, and the descriptor it builds carries the schema. A type *built from*
// `T` cannot be forwarded the same way: its descriptor is assembled here,
// where `T` is unknown, so it has no JSON Schema to send. The error says to
// forward the parameter itself and let the caller name the whole type.
// expect-error: `llm.call<T[]>` cannot send a JSON Schema
import llm from "submilli:llm";

function classifyMany<T>(prompt: string): llm.Completion<T[]> {
  return llm.call<T[]>("claude-haiku-4-5", prompt);
}

function main(): void {
  const s = classifyMany<number>("Classify this.");
  assert(s.ok, "unreachable — the call above does not compile");
}
