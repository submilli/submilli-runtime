// The untyped `call` returns a `Completion` envelope rather than a bare string,
// because a model call has more outcomes than "worked" and "threw". `ok` means
// the model stopped naturally — not merely that nothing was raised — so a
// program that reads `text` without checking `ok` can silently consume a
// truncated answer as though it were complete. The envelope exists to make that
// check unavoidable, and this fixture pins the shape it hands back.
import llm from "submilli:llm";

function main(): void {
  const c = llm.call("claude-haiku-4-5", "Summarize this.");

  // A clean completion reports a natural stop and carries its text.
  assert(c.ok, "a clean completion reports a natural stop");
  assert(c.text !== null, "a successful completion always carries text");
  assert((c.text as string).length > 0, "the completion text reaches the guest");

  // The failure fields are all absent on the success arm — `reason` is what a
  // program branches on, so it must be null rather than an empty string.
  assert(c.reason === null, "a clean completion has no failure reason");
  assert(c.message === null, "a clean completion has no failure message");
  assert(!c.retryable, "there is nothing to retry on the success arm");
  assert(c.finishReason === null, "no raw stop reason is reported for a clean stop");

  // Usage is nullable on both arms: this provider reported none, and null
  // means indeterminate rather than free.
  assert(c.inputTokens === null, "unreported usage stays null rather than zero");
  assert(c.outputTokens === null, "unreported output usage stays null too");
}
