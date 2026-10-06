// Every failure throws, and each lands on the error type the plan maps it to
// (KTD13): a malformed provider response is a TypeError, a provider or
// credential failure is a plain Error with a fixed reason, and no message
// carries the input text.
import embedding from "submilli:embedding";

function main(): void {
  let transport = "";
  try {
    embedding.embed("fixture-embedding", ["FAIL-TRANSPORT secret input"], "document");
    assert(false, "a transport failure throws");
  } catch (e: Error) {
    assert(!((e as unknown) instanceof RangeError), "a provider failure is not an argument error");
    assert(!((e as unknown) instanceof TypeError), "nor a type error");
    transport = e.message;
  }
  assert(transport.indexOf("transport") >= 0, "the reason is a fixed kebab-case word");
  assert(transport.indexOf("secret input") < 0, "and never the input");

  let malformed = "";
  let isTypeError = false;
  try {
    embedding.embed("fixture-embedding", ["FAIL-MALFORMED secret input"], "document");
    assert(false, "a malformed response throws");
  } catch (e: TypeError) {
    isTypeError = true;
    malformed = e.message;
  }
  assert(isTypeError, "a malformed response is a TypeError");
  assert(malformed.indexOf("dimension-mismatch") >= 0, "naming the fixed reason");
  assert(malformed.indexOf("secret input") < 0, "and never the input");

  let unauthorized = "";
  try {
    embedding.embed("fixture-embedding", ["FAIL-UNAUTHORIZED secret input"], "document");
    assert(false, "a rejected credential throws");
  } catch (e: Error) {
    unauthorized = e.message;
  }
  assert(unauthorized.indexOf("credential") >= 0, "the credential failure is named");
  assert(unauthorized.indexOf("secret input") < 0, "and never the input");

  // A failure is not sticky: the next call works.
  assert(embedding.embed("fixture-embedding", ["fine"], "document").count === 1, "execution continues");
}
