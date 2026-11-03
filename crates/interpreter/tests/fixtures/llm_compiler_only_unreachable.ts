// The typed `call<T>` lowers to a compiler-only `#call` that takes the JSON
// Schema and the `value as T` check after the written arguments. No program can
// reach it: `#` is not an identifier character, and the one spelling that could
// name it — a string index on the namespace — does not find it either, so the
// schema and the check stay the compiler's own.
// expect-error: package `submilli:llm` does not export `#call`
// expect-error: package `submilli:llm` does not export `#batch`
import llm from "submilli:llm";

function main(): void {
  const call = llm["#call"]("m", "p", "{}", (v: unknown): unknown => v);
  const batch = llm["#batch"]("m", ["p"], "{}", (v: unknown): unknown => v);
  assert(call !== undefined && batch !== undefined, "unreachable — the calls above do not compile");
}
