enum Level { Low = "low", High = "high" }
function widen(text: Uppercase<Lowercase<string>>): Uppercase<string> { return text; }
function same(text: Uppercase<string>): Uppercase<Uppercase<string>> { return text; }
function main(): void {
  const levels: Uppercase<Level> = "HIGH";
  const codes: keyof { "200": string; "404": string } = "200";
  const picked: Pick<Record<string, number>, string> = { a: 1 };
  const kept: Omit<{ 0: string; a: number }, number> = { a: 1 };
  assert(widen("AB") === "AB" && same("X") === "X", "nested string mappings");
  assert(levels === "HIGH" && codes === "200" && picked["a"] === 1 && kept.a === 1, "enums and keys");
}
