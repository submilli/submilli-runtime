// An error in an arrow's body doesn't hide that the arrow doesn't fit its slot.
// expect-error: unresolved identifier `bogus`
// expect-error: expected `number`, got `(arg0: number) => number`
// expect-error-count: 2
function main(): void {
  const n: number = (a: number) => {
    bogus();
    return a;
  };
}
