// expect-error: `public` must come before `readonly`
// expect-error: `private` must come before `readonly`
// expect-error: write `public readonly <name>`
// expect-error-count: 4
class Point {
  constructor(readonly public x: number, readonly private y: number) {}
}

class Label {
  readonly public text: string = "";
  readonly public static kind: string = "label";
}

function main(): void {}
