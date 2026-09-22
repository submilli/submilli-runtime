// The typed `call<T>` returns `T` itself, not a wrapper — the same trade
// `session.get<T>` makes, swapping the `Completion` envelope for a value that
// has actually been checked. A JSON Schema for `T` is emitted at compile time
// and sent to the provider, and the response is then verified structurally,
// field by field. Both halves are needed: the schema is advisory, so a provider
// may ignore it, and the check is what makes the returned value trustworthy.
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

  // The result is `T` itself: fields are read directly, with no `.text` to
  // parse and no envelope to unwrap.
  assert(s.level === "critical", "a top-level string field arrives checked");
  assert(s.rationale === "payment path down", "every declared field is present");

  // The nested object was checked all the way down, not merely accepted as
  // "some object".
  assert(s.detail.service === "billing", "a nested string field round-trips");
  assert(s.detail.restarts === 3, "a nested number field round-trips as a number");

  // An explicit null satisfies a nullable field — absent and null are the same
  // answer here, and both are legal.
  assert(s.owner === null, "a nullable field accepts an explicit null");

  // The typed form has no `ok` to branch on: that is the trade. A caller who
  // needs the envelope uses the untyped form instead, which still works.
  const untyped = llm.call("claude-haiku-4-5", "Classify this ticket.");
  assert(untyped.ok, "the untyped form still returns the envelope");
  assert(untyped.text !== null, "carrying the raw text for a caller that wants it");

  // `batch<T>` names the shape of the *whole result*, not of one element, so
  // the type argument is an array type.
  const many = llm.batch<Severity[]>("claude-haiku-4-5", ["one", "two"]);
  assert(many.length === 2, "a typed batch returns one checked value per prompt");
  assert(many[0].level === "critical", "each element is checked against the element type");
  assert(many[1].detail.restarts === 3, "including its nested fields");
}
