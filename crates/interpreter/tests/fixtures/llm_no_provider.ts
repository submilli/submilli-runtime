// R12: a runtime with no model provider wired must fail *catchably*, naming the
// model and pointing at the operator. The pure-interpreter path leaves the
// provider unset, so this is the ordinary state of every embedding that has not
// opted into model calls — not an exotic misconfiguration. Trapping instead
// would take down a program whose model call was optional, and a silent empty
// completion would be worse still: it would read as "the model had nothing to
// say" rather than "nothing was ever asked".
//
// No provider is installed for this fixture, so the failure is the real one.
import llm from "submilli:llm";

function main(): void {
  // `call` fails, and the failure is catchable rather than a trap.
  let caught = "";
  try {
    llm.call("claude-haiku-4-5", "Summarize this.");
    assert(false, "a call without a provider must not succeed");
  } catch (e: Error) {
    caught = e.message;
  }
  assert(caught.length > 0, "an unconfigured provider throws a catchable error");

  // The message names the model that was aimed at, so a program calling
  // several models can attribute the failure without matching on prose.
  assert(caught.indexOf("claude-haiku-4-5") >= 0, "the error names the model it dispatched at");

  // And it points at the fix, which belongs to the operator rather than the
  // program author — no amount of program change configures a provider.
  assert(caught.indexOf("operator") >= 0, "the error names who can fix it");

  // R13: the prompt never reaches the error, even on a path where nothing was
  // ever sent anywhere.
  assert(caught.indexOf("Summarize this.") < 0, "the error never echoes the prompt");

  // `batch` fails the same way rather than returning per-element failures —
  // nothing dispatched, so there are no elements to report on.
  let batchCaught = "";
  try {
    llm.batch("claude-haiku-4-5", ["a", "b"]);
    assert(false, "a batch without a provider must not succeed");
  } catch (e: Error) {
    batchCaught = e.message;
  }
  assert(batchCaught.indexOf("claude-haiku-4-5") >= 0, "batch fails catchably and names the model");

  // `models()` cannot answer either: there is no catalog without a provider.
  // It must not report an empty list, which would read as "none configured".
  let modelsCaught = "";
  try {
    llm.models();
    assert(false, "models() without a provider must not report an empty catalog");
  } catch (e: Error) {
    modelsCaught = e.message;
  }
  assert(modelsCaught.length > 0, "models() without a provider throws rather than returning []");

  // The program is still running after catching all three — the point of
  // catchability.
  assert(true, "execution continues past a caught configuration error");
}
