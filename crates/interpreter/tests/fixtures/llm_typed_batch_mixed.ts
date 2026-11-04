// A typed batch checks each response on its own. One element that fits `T`,
// one that is not JSON, and one the provider failed are three independent
// entries: the good one keeps its value, each bad one names its own reason,
// and positions and token counts are preserved. Nothing throws.
import llm from "submilli:llm";

interface Verdict {
  label: string;
  score: number;
}

function main(): void {
  const results = llm.batch<Verdict>("claude-haiku-4-5", ["fits", "prose", "throttled"]);
  assert(results.length === 3, "one entry per prompt, whatever happened to each");

  const fits = results[0];
  assert(fits.ok, "a conforming response is a clean completion");
  if (fits.ok) {
    assert(fits.value.label === "spam" && fits.value.score === 0.9, "its value is the checked T");
    assert(fits.inputTokens === 11 && fits.outputTokens === 12, "with its own token counts");
  }

  const prose = results[1];
  assert(!prose.ok, "prose where JSON was asked for is not a clean completion");
  if (!prose.ok) {
    assert(prose.reason === "invalid-output", "and is invalid output");
    assert(prose.message.indexOf("not JSON") >= 0, "with a message naming the format");
    assert(prose.inputTokens === 21 && prose.outputTokens === 22, "keeping the tokens it was billed");
    assert(prose.text === undefined, "a typed failure carries no text");
  }

  const throttled = results[2];
  assert(!throttled.ok, "a provider failure is not a clean completion");
  if (!throttled.ok) {
    assert(throttled.reason === "rate-limited", "with the provider's own reason");
    assert(throttled.status === 429, "and status");
    assert(throttled.retryable, "a throttle is worth retrying");
  }

  // A `switch` over `ok` covers both arms, and each arm sees its own fields.
  let summary = "";
  for (const r of results) {
    switch (r.ok) {
      case true:
        summary = summary + r.value.label + "|";
        break;
      case false:
        summary = summary + r.reason + "|";
        break;
    }
  }
  assert(summary === "spam|invalid-output|rate-limited|", "narrowing on `ok` reaches each arm's fields");
}
