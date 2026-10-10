// A runtime with no embedding provider refuses with a configuration error
// rather than inventing vectors.
import embedding from "submilli:embedding";

function main(): void {
  let message = "";
  try {
    embedding.embed("fixture-embedding", ["secret input"], "document");
    assert(false, "no provider means no vectors");
  } catch (e: Error) {
    message = e.message;
  }
  assert(message.indexOf("no embedding provider is configured") >= 0, "the error says what is missing");
  assert(message.indexOf("secret input") < 0, "and never the input");

  let listing = "";
  try {
    embedding.models();
    assert(false, "models() needs a provider too");
  } catch (e: Error) {
    listing = e.message;
  }
  assert(listing.indexOf("no embedding provider is configured") >= 0, "discovery reports the same");
}
