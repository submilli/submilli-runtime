// R11, the per-element half: `reason` is a closed set of nine kebab-case
// values, and it is what a program branches on. The spelling is the wire form,
// so it must not drift — a program matching `"content-filtered"` to decide
// whether to fall back is relying on this exact string.
//
// The set is closed on purpose. A provider that meets a stop it cannot place
// reports `"incomplete"` and puts its own raw spelling in `finishReason` rather
// than widening the vocabulary, so a `switch` over these nine stays exhaustive
// across providers and SDK versions. That is why `reason` and `finishReason`
// are separate fields: one is the closed set, the other is the open string.
//
// This is *not* the same taxonomy as the dispatch-level errors (`llm_denied`,
// `llm_no_provider`, the budget fixtures). Those answer "the request never
// ran"; these answer "the request ran and this element is not clean". Neither
// is a subset of the other.
import llm from "submilli:llm";

// Every element of this batch is a failure; narrowing once here keeps each
// assertion below on the failure arm.
function failed(r: llm.Completion<string>): llm.Failure {
  if (r.ok) throw new Error("every element in this batch is a failure of some kind");
  return r;
}

function main(): void {
  // One prompt per category, in the order the fake answers them.
  const results = llm.texts("claude-haiku-4-5", [
    "a", "b", "c", "d", "e", "f", "g", "h", "i",
  ]);
  assert(results.length === 9, "every category is represented");

  // Not one element is `ok` — each is a distinct way of not being clean.
  const failures: llm.Failure[] = [];
  for (const r of results) {
    const f = failed(r);
    assert(f.reason.length > 0, "a failed element always names its category");
    assert(f.message.length > 0, "and carries a classification message");
    failures.push(f);
  }

  // Truncated: hit the output ceiling mid-answer, and the partial text is
  // retained — the whole reason the failure arm has a `text` field.
  assert(failures[0].reason === "truncated", "a length stop is `truncated`");
  assert(failures[0].text !== undefined, "a truncated element keeps the text it produced");
  assert(failures[0].finishReason === "length", "the provider's raw stop reason travels separately");

  // Content-filtered: stopped or redacted by a safety filter, and also still
  // carrying partial text.
  assert(failures[1].reason === "content-filtered", "a safety stop is `content-filtered`");
  assert(failures[1].text !== undefined, "a filtered element can still carry partial text");

  // Invalid output: the model answered, but not usably. Nothing was cut off —
  // what arrived was wrong, which is why it is distinct from `truncated`.
  assert(failures[2].reason === "invalid-output", "an unusable answer is `invalid-output`");

  // Rate limited: throttled, and worth retrying after a wait.
  assert(failures[3].reason === "rate-limited", "a throttle is `rate-limited`");
  assert(failures[3].status === 429, "the observed HTTP status reaches the guest");
  assert(failures[3].retryable, "a throttled element is retryable");

  // Request rejected: the request as sent will never succeed, so retrying it
  // unchanged cannot help. The retryable flag is what encodes that.
  assert(failures[4].reason === "request-rejected", "a refused request is `request-rejected`");
  assert(failures[4].status === 400, "with its status preserved");
  assert(!failures[4].retryable, "a rejected request is not worth retrying unchanged");

  // Provider unavailable: reachable but could not serve it; the same bytes may
  // succeed later, which is exactly what separates it from a rejection.
  assert(failures[5].reason === "provider-unavailable", "a 5xx is `provider-unavailable`");
  assert(failures[5].status === 503, "with its status preserved");
  assert(failures[5].retryable, "an unavailable provider may serve the same request later");

  // Transport: the connection failed after dispatch began, so no status was
  // ever observed. An absent status is meaningful rather than missing — it is
  // what distinguishes a dead connection from a provider that answered.
  assert(failures[6].reason === "transport", "a mid-flight connection failure is `transport`");
  assert(failures[6].status === undefined, "a transport death never observed a status");
  assert(!("status" in failures[6]), "and the absent status is an absent key");

  // Each failure carries exactly its present fields, in canonical order.
  assert(
    Object.keys(failures[0]).join(",") === "finishReason,message,ok,reason,retryable,text",
    "a truncated element carries its stop reason and text",
  );
  assert(
    Object.keys(failures[3]).join(",") === "message,ok,reason,retryable,status",
    "a throttled element carries its status and nothing absent",
  );

  // Cancelled: abandoned on this side — deadline, shutdown, abort — rather
  // than on the wire, which is why it is not folded into `transport`.
  assert(failures[7].reason === "cancelled", "an abandoned element is `cancelled`");

  // Incomplete: a stop this runtime does not classify. The closed set does not
  // grow; the raw spelling travels in `finishReason` for diagnosis.
  assert(failures[8].reason === "incomplete", "an unclassified stop is `incomplete`");
  assert(
    failures[8].finishReason === "provider-specific-stop",
    "and the provider's own spelling is preserved verbatim for diagnosis",
  );

  // Every category is distinct — a taxonomy that collapsed two would make a
  // branch unreachable without any assertion above noticing.
  let distinct = 0;
  for (let i = 0; i < failures.length; i = i + 1) {
    let isFirst = true;
    for (let j = 0; j < i; j = j + 1) {
      if (failures[j].reason === failures[i].reason) isFirst = false;
    }
    if (isFirst) distinct = distinct + 1;
  }
  assert(distinct === 9, "all nine categories are distinct values");

  // R13: no classification message quotes a prompt or a completion.
  for (const f of failures) {
    assert(f.message.indexOf("answer") < 0, "a classification message never echoes completion text");
  }
}
