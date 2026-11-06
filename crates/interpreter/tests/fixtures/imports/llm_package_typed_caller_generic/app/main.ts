import llm from "submilli:llm";
import { classify, classifyAll } from "@test/triage";

interface Label {
  label: string;
}

interface Scored {
  score: number;
}

function main(): void {
  // The script's descriptor for `Label` carries the schema and the check.
  const fits = classify<Label>("claude-haiku-4-5", "fits");
  assert(fits.ok && fits.value.label === "spam", "a conforming response is the checked Label");

  const prose = classify<Label>("claude-haiku-4-5", "prose please");
  assert(!prose.ok && prose.reason === "invalid-output", "prose is invalid output");

  // The same answer checked against a different type the caller names.
  const scored = classify<Scored>("claude-haiku-4-5", "fits");
  assert(!scored.ok && scored.message.indexOf("does not match `Scored`") >= 0, "names the caller's type");

  const all = classifyAll<Label>("claude-haiku-4-5", ["fits", "prose please"]);
  assert(all.length === 2 && all[0].ok && !all[1].ok, "each element is checked on its own");

  // A type with no JSON Schema is refused when the call runs, before anything
  // is sent: only the caller knew the type.
  let refused = "";
  try {
    classify<Uint8Array>("claude-haiku-4-5", "fits");
  } catch (e: TypeError) {
    refused = e.message;
  }
  assert(refused.indexOf("has no JSON Schema") >= 0, "a schema-less type is a TypeError");
}
