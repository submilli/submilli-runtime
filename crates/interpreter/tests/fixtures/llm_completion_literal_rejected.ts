// A completion is a plain object, so an object literal of the same shape is
// one — and one of a different shape is not. The arms are checked as any
// declared interface is: a required field cannot be missing, an unknown field
// is refused, and each field's type holds.
// expect-error: missing required field `retryable`
// expect-error: unknown field `extra`
// expect-error: field `value`: expected `string`, got `number`
// expect-error: field `reason`: expected `"cancelled"
import llm from "submilli:llm";

function text(r: llm.Completion<string>): string {
  return r.ok ? r.value : "";
}

function main(): void {
  const missing: llm.Completion<string> = { ok: false, reason: "cancelled", message: "m" };
  const extra: llm.Completion<string> = { ok: true, value: "v", extra: 1 };
  const wrongValue: llm.Completion<string> = { ok: true, value: 42 };
  const wrongReason: llm.Completion<string> = { ok: false, reason: "nope", message: "m", retryable: false };

  assert(text(missing) === "", "unreachable — the literals above do not compile");
  assert(text(extra) === "", "unreachable");
  assert(text(wrongValue) === "", "unreachable");
  assert(text(wrongReason) === "", "unreachable");
}
