// `llm.call<T>()` sends a JSON Schema for `T` to the provider, and that schema
// is inlined in full — no `$ref`, no `$defs`. Google's provider strips `$ref`
// and silently degrades the referenced subschema to "any", so a schema that
// leaned on refs would constrain nothing while still looking like a typed call.
// Inlining is therefore mandatory, and every type that cannot be inlined into
// the safe subset has to fail here, at compile time, rather than reach a
// provider as a schema that quietly permits anything.
//
// The gate that decides this is the schema emitter's own, not the `as` cast
// gate. The two deliberately disagree: a cast to `unknown` is a no-op widen and
// needs no runtime test, but a schema for `unknown` could only be `{}` — the
// same degeneration to "any". Each rejection below names the offending field,
// so the author knows which property to change, not merely that the result type
// is unsupported.
// expect-error: field `handler`
// expect-error: field `payload`
// expect-error: field `size`
// expect-error: field `blob`
// expect-error: field `next`
import llm from "submilli:llm";

interface WithFunction {
  id: string;
  // A function has no JSON form at all. This shape compiles as an `as` target
  // today, which is exactly why the schema needs a gate of its own.
  handler: (n: number) => number;
}

interface WithUnknown {
  id: string;
  // `unknown` admits every value, so its only possible schema is `{}`. Omitting
  // the key instead would change whether it is required, so the answer is to
  // reject and ask for the shape the caller actually expects.
  payload: unknown;
}

interface WithBigInt {
  // `bigint` has no JSON number form — the reason names `bigint` specifically,
  // separately from `Uint8Array`, so the suggested fix differs.
  size: bigint;
}

interface WithBytes {
  // `Uint8Array` has no JSON form either, and wants a string encoding.
  blob: Uint8Array;
}

// A type reachable from itself has no finite inlining. `$ref` is not an escape
// hatch here, so recursion is a compile error rather than a degraded schema.
interface Node {
  value: string;
  next: Node | null;
}

function main(): void {
  // Each call is rejected for its own field, before any request is made.
  const withFunction = llm.call<WithFunction>("claude-haiku-4-5", "describe a handler");
  const withUnknown = llm.call<WithUnknown>("claude-haiku-4-5", "describe a payload");
  const withBigInt = llm.call<WithBigInt>("claude-haiku-4-5", "describe a size");
  const withBytes = llm.call<WithBytes>("claude-haiku-4-5", "describe a blob");
  const recursive = llm.call<Node>("claude-haiku-4-5", "describe a linked list");

  assert(withFunction.id === "", "unreachable — the call above does not compile");
  assert(withUnknown.id === "", "unreachable");
  assert(withBigInt.size === 0n, "unreachable");
  assert(withBytes.blob.length === 0, "unreachable");
  assert(recursive.value === "", "unreachable");
}
