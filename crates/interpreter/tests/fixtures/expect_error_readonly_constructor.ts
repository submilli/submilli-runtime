// expect-error: a constructor cannot be `readonly`
class C {
  readonly constructor() {}
}
function main(): void {}
