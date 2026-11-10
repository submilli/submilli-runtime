import { runOptional, runText } from "@test/json";

function main(): void {
  assert(runOptional(Number) === undefined, "a constructor in an optional place");
  let caught = false;
  try {
    runText(Number);
  } catch (e) {
    assert(e.name === "MissingValueError", e.message);
    caught = true;
  }
  assert(caught, "a constructor in a string place throws");
}
