// `description` and `contextWindow` are each `null` when the operator declared
// none, and absent means *unknown* rather than zero. Models are blueprint
// declared, so an operator may legitimately name a model without asserting
// anything else about it — and there is no fallback table to fill the gap.
//
// This is the trap the field's nullability exists to force: a program sizing
// chunks against `contextWindow` must drop a model that declared none rather
// than treat it as `0` (which would reject every prompt) or guess a default
// (which would overflow a small model's window). Filtering on either field
// drops the undeclared models, and an empty result means "none declared one",
// not "none are configured".
import llm from "submilli:llm";

function main(): void {
  const models = llm.models();
  assert(models.length === 2, "both declared models are listed");

  // Walk the listing and assert each model's own optionality in place —
  // ordering is not the claim under test, so each is found by name.
  let sawDescribed = false;
  let sawBare = false;
  for (const m of models) {
    if (m.name === "claude-haiku-4-5") {
      sawDescribed = true;
      // The fully declared model carries both optional fields.
      assert(m.description !== null, "a declared description reaches the guest");
      assert((m.description as string).length > 0, "and is non-empty");
      assert(m.contextWindow !== null, "a declared context window reaches the guest");
      assert((m.contextWindow as number) === 200000, "with the operator's own number");
    }
    if (m.name === "bare-model") {
      sawBare = true;
      // The name-only model reports null for both — not "", and not 0.
      assert(m.description === null, "an undeclared description is null, not an empty string");
      assert(m.contextWindow === null, "an undeclared context window is null, not zero");
    }
  }
  assert(sawDescribed, "the fully declared model is listed");
  assert(sawBare, "a model declared by name alone is still listed");

  // The chunk-sizing decision from the worked example: a model that declared no
  // window drops out, which is correct — you cannot size against a number
  // nobody asserted.
  // Carrying the window as a plain local keeps the narrowing off a field path.
  let widestName = "";
  let widestWindow = 0;
  for (const m of models) {
    const window = m.contextWindow;
    if (window === null) continue;
    if (widestName === "" || window > widestWindow) {
      widestName = m.name;
      widestWindow = window;
    }
  }
  assert(widestName === "claude-haiku-4-5", "the undeclared model dropped out of sizing");
  assert(widestWindow === 200000, "the surviving window is the declared one");

  // Dropping it from a sizing decision does not make it uncallable: a null
  // window means unknown, not unusable.
  const c = llm.call("bare-model", "Summarize this.");
  assert(c.ok, "a model that declared no context window is still callable");
}
