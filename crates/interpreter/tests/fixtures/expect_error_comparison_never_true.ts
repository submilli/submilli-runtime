// A comparison that can never be true is rejected whichever side holds the
// literal, as tsc rejects it (TS2367, TS2678): an enum against a value none of
// its members has, two different literals, and a template of constants against
// a string it doesn't spell. Members of different enums never compare, even
// with equal values.
// expect-error: expected `"bar"`, got `"foo"`
// expect-error: expected `Color.Red`, got `Shade`
// expect-error: case label of type `Shade.Dark` is not compatible with switch discriminant of type `Color`
// expect-error-count: 17
enum Color { Red = 1, Green = 2 }
enum Mode { On = "on", Off = "off" }
enum Shade { Dark = 1, Light = 2 }

function check(color: Color, mode: Mode, foo: "foo", flag: true, shade: Shade, maybeShade: Shade | null): void {
  const a = "bar" === foo;
  const b = foo === "bar";
  const c = color === 0;
  const d = 3 !== color;
  const e = mode === "auto";
  const f = Color.Red === Color.Green;
  const g = flag === false;
  const h = `id${1}` === "id2";
  const i = Color.Red === shade;
  const l = maybeShade !== Color.Green;
  const j = Mode.On === "off";
  const k = `${-0}` === "-0";
  switch (12) {
    case 5:
      break;
  }
  switch (color) {
    case 0:
      break;
    default:
      break;
  }
  switch (`v${2}`) {
    case "v3":
      break;
  }
  switch (mode) {
    case Color.Red:
      break;
    default:
      break;
  }
  switch (color) {
    case Shade.Dark:
      break;
    default:
      break;
  }
}

function main(): void {}
