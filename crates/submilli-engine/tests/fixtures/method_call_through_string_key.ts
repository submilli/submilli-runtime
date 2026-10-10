// `o["m"](…)` calls method `m` exactly as `o.m(…)` does — the spelling that reaches
// method names an identifier can't express.
type Api = { "get-name"(): string; count: number };

function main(): void {
  assert("x"["toUpperCase"]() === "X", "a string method");
  const xs = [3, 1, 2];
  assert(xs["join"]("-") === "3-1-2", "an array method with an argument");
  const api: Api = {
    "get-name"(): string {
      return "n";
    },
    count: 2,
  };
  assert(api["get-name"]() === "n", "a method whose name is not an identifier");
  assert(api["count"] === api.count, "field reads are unchanged");
  assert(Math["floor"](1.5) === 1, "a namespace function");
}
