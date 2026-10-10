// An alias the provider does not serve is a catchable error that names the
// aliases that are available, so the program (or its author) can correct it.
import embedding from "submilli:embedding";

function main(): void {
  let message = "";
  try {
    embedding.embed("not-declared", ["secret input"], "document");
    assert(false, "an unknown alias must not dispatch");
  } catch (e: Error) {
    message = e.message;
  }
  assert(message.indexOf("not-declared") >= 0, "the error names the alias that was asked for");
  assert(message.indexOf("fixture-embedding") >= 0, "and an available alias");
  assert(message.indexOf("gemini-embedding-001") >= 0, "and the others");
  assert(message.indexOf("secret input") < 0, "never the input text");
}
