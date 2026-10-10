// deny-embedding-model: secret-embedding
// A `model` filter that denies one alias hides it from `models()` and denies it
// in `embed`, while the other aliases keep working. The same filter shapes the
// aliases an unknown-alias error offers.
import embedding from "submilli:embedding";

function main(): void {
  const names: string[] = [];
  for (const m of embedding.models()) {
    names.push(m.name);
  }
  assert(names.length === 2, "the denied alias is omitted from the listing");
  assert(names.indexOf("secret-embedding") < 0, "by name");

  let denied = false;
  try {
    embedding.embed("secret-embedding", ["x"], "document");
  } catch (e: PermissionDeniedError) {
    denied = true;
    assert(e.capability === "embedding.embed", "embed is denied under the same capability");
  }
  assert(denied, "the filtered alias is denied in embed");

  assert(embedding.embed("fixture-embedding", ["x"], "document").count === 1, "an allowed alias still embeds");

  let message = "";
  try {
    embedding.embed("no-such-alias", ["x"], "document");
  } catch (e: Error) {
    message = e.message;
  }
  assert(message.indexOf("fixture-embedding") >= 0, "an unknown-alias error names the aliases the caller may use");
  assert(message.indexOf("secret-embedding") < 0, "and not the ones the policy hides");
}
