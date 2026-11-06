// SUB-1830: as in tsc, `done` on an `IteratorResult` is `boolean | undefined`,
// since a yielded result may leave it out. Compare it with `true` instead.
// expect-error: expected `boolean`, got `boolean | undefined`
function main(): void {
  const done: boolean = [1].values().next().done;
}
