// A top-level binding named like a prelude namespace hides it, as a local one does:
// `Math.floor` and `JSON.stringify` here are the user's, in both spellings.
const Math = {
  floor(x: number): number {
    return 99;
  },
};

const JSON = {
  stringify(x: number): string {
    return "mine";
  },
};

function main(): void {
  assert(Math.floor(1.5) === 99, "the dotted spelling");
  assert(Math["floor"](1.5) === 99, "the string-key spelling");
  assert(JSON.stringify(1) === "mine", "JSON, whose calls are checked separately");
  assert(JSON["stringify"](1) === "mine", "JSON through a string key");
}
