// deny-llm-model: internal-secret-model
// `models()` filters each candidate
// under the same `model` filter that gates calling. A listing must therefore
// never offer a model the caller would be refused at `call` time — otherwise
// discovery becomes a way to enumerate the operator's catalog behind the
// filter's back, and a program that picks from the list gets a denial it had
// every reason to think impossible.
//
// The filtering is silent by construction. Nothing derived from the hidden
// candidates reaches the guest: no count, no index, no gap. The visible list is
// byte-identical to what a runtime configured with only those models returns,
// which is what keeps the absent model unobservable rather than merely unnamed.
import llm from "submilli:llm";

function main(): void {
  const models = llm.models();

  // The two permitted models survive; the filtered one is simply not there.
  assert(models.length === 2, "the listing contains only the permitted candidates");

  let sawHaiku = false;
  let sawSonnet = false;
  for (const m of models) {
    assert(m.name !== "internal-secret-model", "a denied candidate never appears in the listing");
    if (m.name === "claude-haiku-4-5") sawHaiku = true;
    if (m.name === "claude-sonnet-5") sawSonnet = true;
  }
  assert(sawHaiku, "a permitted model is listed");
  assert(sawSonnet, "every permitted model is listed");

  // The filter is the same one that gates calling, so the hidden model is
  // refused at `call` time too — discovery and dispatch agree.
  let denied = "";
  try {
    llm.call("internal-secret-model", "Summarize this.");
    assert(false, "a filtered model must not be callable");
  } catch (e: PermissionDeniedError) {
    denied = e.capability;
  }
  assert(denied === "llm.call", "the hidden model is refused at call time under the same capability");

  // And a permitted model really is callable — the filter hides, it does not
  // disable everything.
  const c = llm.call("claude-haiku-4-5", "Summarize this.");
  assert(c.ok, "a permitted model still dispatches");
}
