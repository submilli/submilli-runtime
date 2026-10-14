// A constant bitwise substitution spells its value, so another literal is
// rejected.
// expect-error: expected `"1"`, got `"0"`
// expect-error-count: 1
function main(): void {
  const wrong: "1" = `${3 & 4}`;
}
