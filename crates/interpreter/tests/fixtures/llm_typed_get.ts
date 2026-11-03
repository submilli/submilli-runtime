// The typed `call<T>` returns `Completion<T>`: the same envelope as the untyped
// form, with `value` holding a `T` that has actually been checked. A JSON
// Schema for `T` is emitted at compile time and sent to the provider, and the
// response is then verified structurally, field by field. Both halves are
// needed: the schema is advisory, so a provider may ignore it, and the check is
// what makes `value` trustworthy.
//
// Nothing is coerced. A value that reaches the guest as `T` really does have
// `T`'s shape, all the way down through nested objects and optional fields.
import llm from "submilli:llm";

interface Detail {
  service: string;
  restarts: number;
}

interface Severity {
  level: string;
  rationale: string;
  // A nested interface is reduced to its object shape and checked in depth.
  detail: Detail;
  // A nullable field accepts a present value or an explicit null.
  owner: string | null;
}

function main(): void {
  const s = llm.call<Severity>("claude-haiku-4-5", "Classify this ticket.");

  // The envelope is the same one the untyped form returns, so `ok` is checked
  // first; `value` is then `T` itself, with no `.text` to parse.
  assert(s.ok, "a conforming response is a clean completion");
  if (!s.ok) return;
  const v = s.value;
  assert(v.level === "critical", "a top-level string field arrives checked");
  assert(v.rationale === "payment path down", "every declared field is present");

  // The nested object was checked all the way down, not merely accepted as
  // "some object".
  assert(v.detail.service === "billing", "a nested string field round-trips");
  assert(v.detail.restarts === 3, "a nested number field round-trips as a number");

  // An explicit null satisfies a required nullable field; an absent field does not.
  assert(v.owner === null, "a nullable field accepts an explicit null");

  // The untyped form hands back the raw text in `value` for a caller that
  // wants to parse it some other way.
  const untyped = llm.call("claude-haiku-4-5", "Classify this ticket.");
  assert(untyped.ok && untyped.value.indexOf("critical") > 0, "the untyped form carries the raw text");

  // `batch<T>` names the shape of *one* response, so each element is a
  // `Completion<T>` checked on its own.
  const many = llm.batch<Severity>("claude-haiku-4-5", ["one", "two"]);
  assert(many.length === 2, "a typed batch returns one checked completion per prompt");
  const first = many[0];
  assert(first.ok && first.value.level === "critical", "each element is checked against the element type");
  const second = many[1];
  assert(second.ok && second.value.detail.restarts === 3, "including its nested fields");

  // A typed call inside a closure carries its check the same way: the
  // compiler's check closure nests inside the program's own.
  const levels = ["a", "b"].map((prompt) => {
    const c = llm.call<Severity>("claude-haiku-4-5", prompt);
    return c.ok ? c.value.level : c.reason;
  });
  assert(levels.join(",") === "critical,critical", "a typed call works inside a closure");
}
