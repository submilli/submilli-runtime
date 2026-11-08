enum Color { Red = "red", Green = "green", Blue = "blue" }
function main(): void {
  const b: Exclude<"a" | "b" | "c", "a" | "c"> = "b";
  const s: Extract<"a" | 1 | true, string | boolean> = true;
  const off: Exclude<boolean, true> = false;
  const warm: Exclude<Color, Color.Green | Color.Blue> = Color.Red;
  const wide: Exclude<string | number, number> = "w";
  const n: NonNullable<string | null | undefined> = "n";
  const nothing: NonNullable<null | undefined>[] = [];
  const any: NonNullable<unknown> = 0;
  const id: NoInfer<number> = 4;
  assert(b === "b" && s === true && !off && warm === Color.Red && wide === "w", "exclude and extract");
  assert(n === "n" && nothing.length === 0 && any === 0 && id === 4, "non-nullable and no-infer");
}
