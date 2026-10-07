// A literal operand is compared as its own literal type on either side, an enum
// member as its value, and a template of constants as the string it spells.
// Comparisons that can be true still compile and run.
enum Color { Red = 1, Green = 2 }
enum Mode { On = "on", Off = "off" }
enum Sign { Zero = -0, Minus = -1 }

function colorName(color: Color): string {
  switch (color) {
    case 1:
      return "red";
    case Color.Green:
      return "green";
    default:
      return "other";
  }
}

function isGreen(color: Color): boolean {
  return 2 === color;
}

function isRed(color: Color): boolean {
  return color == 1;
}

// The discriminant is the literal `"v2"`, so its one case covers it.
function matchesFoldedLabel(): boolean {
  switch (`v${2}`) {
    case "v2":
      return true;
  }
}

function main(): void {
  const foo: "foo" | "bar" = "foo";
  assert("foo" === foo, "literal on the left");
  assert(isGreen(Color.Green) && !isRed(Color.Green), "enum against one of its values");
  assert(Color.Red === 1 && Mode.On === "on", "enum member against its value");
  assert(`id${1}` === "id1", "folded template");
  assert(`${`a${1}`}b` === "a1b" && `${1e21}` === "1e+21", "nested template and number format");
  assert(Sign.Zero === 0 && 0 === Sign.Zero, "`-0` member against `0`");
  assert(Sign.Minus === -1, "negative member against its value");
  assert((Color.Red) == 1 && !(Mode.Off != "off"), "parenthesised operand, loose operators");
  assert(colorName(Color.Red) === "red", "number label on an enum discriminant");
  assert(colorName(Color.Green) === "green", "enum member label");
  assert(matchesFoldedLabel(), "template discriminant matches its folded label");
}
