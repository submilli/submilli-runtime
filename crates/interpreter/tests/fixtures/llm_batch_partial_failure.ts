// R3: one failing element must not discard the ones that succeeded. The
// alternative — surfacing a whole-batch error — would throw away sibling
// completions that already ran and were already billed, so the caller pays for
// work it never receives and cannot tell which prompts to retry. Per-element
// outcomes are what make a partial batch recoverable.
//
// The second half matters just as much: a failed element still carries its
// text. A truncated completion is `ok: false` *and* holds a usable partial
// answer, so the reflexive `if (!r.ok) continue` silently drops output the
// model did produce. This fixture pins both halves.
import llm from "submilli:llm";

function main(): void {
  const prompts = ["alpha", "beta", "gamma"];
  const results = llm.batch("claude-haiku-4-5", prompts);

  // The batch returns rather than throwing, and keeps its positional shape
  // even though an element inside it failed.
  assert(results.length === 3, "a partial failure still returns one result per prompt");

  // The siblings survive, with their own text intact.
  assert(results[0].ok, "the element before the failure survives");
  assert((results[0].text as string).indexOf("answer 0") === 0, "and keeps its own answer");
  assert(results[2].ok, "the element after the failure survives");
  assert((results[2].text as string).indexOf("answer 2") === 0, "and keeps its own answer");

  // The failed element reports why, in the closed kebab-case vocabulary a
  // program branches on.
  const failed = results[1];
  assert(!failed.ok, "the failing element reports a non-natural stop");
  assert(failed.reason === "truncated", "the reason names the closed-set category");

  // The partial text survives the failure — this is the whole point of the
  // failure arm keeping a `text` field.
  assert(failed.text !== null, "a truncated element still carries the text it produced");
  assert((failed.text as string).length > 0, "and that text is non-empty");

  // The raw provider stop reason travels separately, for diagnosis only.
  assert(failed.finishReason === "length", "the provider's own stop reason is preserved verbatim");

  // A truncated completion is not worth retrying unchanged.
  assert(!failed.retryable, "re-sending an identical prompt would truncate again");

  // Counting only `ok` elements undercounts the usable answers — the exact
  // mistake the `ok` rule exists to surface.
  let clean = 0;
  let usableText = 0;
  for (const r of results) {
    if (r.ok) clean = clean + 1;
    if (r.text !== null) usableText = usableText + 1;
  }
  assert(clean === 2, "two elements stopped naturally");
  assert(usableText === 3, "but all three produced text worth reading");
}
