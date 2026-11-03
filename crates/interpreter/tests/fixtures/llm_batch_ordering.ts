// `batch` results are positional: `result[i]` is the outcome of `prompts[i]`.
// This is contractual rather than incidental — fan-out happens host-side at a
// bounded concurrency, so completion order is not dispatch order, and a caller
// correlating answers back to inputs has nothing but the index to do it with. A
// provider that returned results in completion order would silently mismatch
// every answer to the wrong prompt, which no assertion on content alone would
// catch. The fake here answers with its own index, so a reordering is visible.
import llm from "submilli:llm";

function text(r: llm.Completion<string>): string {
  return r.ok ? r.value : "";
}

function main(): void {
  const prompts = ["first", "second", "third", "fourth"];
  const results = llm.batch("claude-haiku-4-5", prompts);

  // One result per prompt, never fewer and never more.
  assert(results.length === prompts.length, "batch returns one result per prompt");

  // Each result sits at its own prompt's index.
  for (let i = 0; i < results.length; i = i + 1) {
    const r = results[i];
    assert(r.ok, "each element of a clean batch stopped naturally");
    assert(text(r).indexOf("answer " + String(i)) === 0, "result i is the outcome of prompt i");
  }

  // The fake encodes each prompt's byte length, so a swapped pair would be
  // caught even if the index prefix happened to line up.
  assert(text(results[0]).indexOf("5 bytes") > 0, "prompt 0's own length comes back at 0");
  assert(text(results[3]).indexOf("6 bytes") > 0, "prompt 3's own length comes back at 3");

  // A single-element batch is the degenerate case of the same rule.
  const one = llm.batch("claude-haiku-4-5", ["only"]);
  assert(one.length === 1, "a one-prompt batch returns one result");
  assert(text(one[0]).indexOf("answer 0") === 0, "the single result is prompt 0's");

  // An empty batch dispatches nothing and returns nothing.
  const none = llm.batch("claude-haiku-4-5", []);
  assert(none.length === 0, "an empty batch returns an empty list");

  // A completion is a plain object, so an object literal of the same shape is
  // one too, on either arm.
  assert(text({ ok: true, value: "literal" }) === "literal", "a hand-built success is a completion");
  const cancelled: llm.Completion<string> = { ok: false, reason: "cancelled", message: "abandoned", retryable: false };
  assert(text(cancelled) === "" && !cancelled.ok && cancelled.reason === "cancelled", "a hand-built failure is a completion");
}
