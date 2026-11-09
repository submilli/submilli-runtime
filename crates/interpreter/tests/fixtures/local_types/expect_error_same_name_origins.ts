// SUB-2191: the declarations and a concrete shape difference disambiguate T.
// expect-error-count: 1
// expect-error: expected `T` declared at line 5, got `T` declared at line 7
// expect-error: missing field(s): `card_id`
interface T { card_id: string; }
function main(): void {
  interface T { other_id: string; }
  const local: T = { other_id: "x" };
  take(local);
}
function take(value: T): void {}
