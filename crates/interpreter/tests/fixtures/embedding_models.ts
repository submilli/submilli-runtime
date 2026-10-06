// `models()` lists the aliases with the facts a chunker sizes inputs by.
// R3: dimensions, input limit in tokens and in bytes, identity.
// The byte limit is `3 x maxInputTokens` for an ordinary alias and the
// tighter Google bound (token limit less special tokens) for gemini-001.
import embedding from "submilli:embedding";

function main(): void {
  const all = embedding.models();
  assert(all.length === 3, "every candidate the caller may use is listed");

  let normalSeen = false;
  let geminiSeen = false;
  for (const m of all) {
    assert(m.dimensions === 8, "dimensions are reported");
    assert(m.identity.length > 0, "every alias reports its identity");
    assert(m.maxInputBytes > 0, "and its byte limit");
    if (m.name === "fixture-embedding") {
      normalSeen = true;
      assert(m.maxInputTokens === 512, "the token limit is reported");
      assert(m.maxInputBytes === 3 * 512, "a non-Google alias allows 3 x maxInputTokens bytes");
      assert(m.description === "Deterministic test embeddings.", "the description reaches the program");
    }
    if (m.name === "gemini-embedding-001") {
      geminiSeen = true;
      assert(m.maxInputTokens === 2048, "the Google token limit is reported");
      assert(m.maxInputBytes === 2032, "gemini-001 is bounded at 2,032 bytes, not 3 x 2,048");
      assert(m.description === null, "an undeclared description is null");
    }
  }
  assert(normalSeen, "the ordinary alias is listed");
  assert(geminiSeen, "the gemini-001 alias is listed");

  // The identity discovery reports is the one results carry.
  const r = embedding.embed("fixture-embedding", ["x"], "document");
  let discovered = "";
  for (const m of all) {
    if (m.name === "fixture-embedding") {
      discovered = m.identity;
    }
  }
  assert(r.identity === discovered, "discovery and results carry the same identity");
}
