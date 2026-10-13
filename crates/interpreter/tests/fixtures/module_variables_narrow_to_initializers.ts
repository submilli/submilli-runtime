// A module-level variable declared as a union starts narrowed to its
// initializer, as a local one does and as tsc has it: an annotated `const`,
// and each binding of a destructured `let` or `const`.
type RexOrRaptor = "t-rex" | "raptor";

const count: string | number = 1;
const asNumber: number = count;

let [greeting, dinosaur]: ["I'm", RexOrRaptor] = ["I'm", "t-rex"];
const rex: "t-rex" = dinosaur;

const [, other]: ["I'm", RexOrRaptor] = ["I'm", "raptor"];
const raptor: "raptor" = other;

let { kind }: { kind: RexOrRaptor } = { kind: "raptor" };
const alsoRaptor: "raptor" = kind;

function main(): void {
  assert(asNumber === 1, "an annotated const");
  assert(greeting === "I'm" && rex === "t-rex", "a destructured let");
  assert(raptor === "raptor", "a destructured const");
  assert(alsoRaptor === "raptor", "a destructured object");
}
