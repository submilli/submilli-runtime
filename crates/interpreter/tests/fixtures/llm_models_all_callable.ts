// The contract `models()` owes its caller: every model it returns is one that
// caller may actually call. Discovery that could offer an unusable name would
// be worse than no discovery at all — a program picking from the list would hit
// a permission denial at dispatch, on a name the runtime itself had just
// recommended, and would have no way to tell which entries were real.
//
// This is the positive case of filtering; `llm_models_filtered` covers
// the negative half. Asserting only that denied models are hidden would leave
// the vacuous case passing — a `models()` that returned nothing at all also
// hides every denied model. So this fixture walks the list and dispatches at
// each entry, which fails if the list is empty for the wrong reason.
import llm from "submilli:llm";

function main(): void {
  const models = llm.models();

  // The listing is non-empty here, so the loop below is not vacuously true.
  assert(models.length > 0, "this runtime serves at least one model to this caller");

  // Every listed name dispatches without a permission denial.
  let called = 0;
  for (const m of models) {
    // A listed name is exactly the spelling `call` expects — no prefix to
    // strip, no provider to qualify it with.
    const c = llm.call(m.name, "Summarize this.");
    assert(c.ok, "a listed model completes when called");
    called = called + 1;
  }
  assert(called === models.length, "every listed model was callable by this caller");

  // The same names work through `batch`, which shares the capability and the
  // filter — discovery is not `call`-specific.
  for (const m of models) {
    const results = llm.batch(m.name, ["one", "two"]);
    assert(results.length === 2, "a listed model accepts a batch too");
  }

  // A name that is *not* in the listing is not callable: the catalog is
  // authoritative, so an undeclared model is refused rather than attempted.
  let unknown = "";
  try {
    llm.call("no-such-model", "Summarize this.");
    assert(false, "an unlisted model must not dispatch");
  } catch (e: Error) {
    unknown = e.message;
  }
  assert(unknown.length > 0, "an undeclared model throws catchably");
  assert(unknown.indexOf("no-such-model") >= 0, "the rejection names the model that was asked for");
  assert(unknown.indexOf("Summarize this.") < 0, "and never echoes the prompt");
}
