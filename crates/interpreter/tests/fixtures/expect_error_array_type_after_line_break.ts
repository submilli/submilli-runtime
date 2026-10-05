// A line break ends a type before `[`, so `[]` on the next line is not an
// array suffix there.
// expect-error: expected `,` or `]`
// expect-error: expected expression
// expect-error-count: 2
function first<T>(items: T[]): T {
  return items[0];
}

const tuple: [number
  []] = [1];
const value = first<number
  []>([1, 2]);

function main(): void {}
