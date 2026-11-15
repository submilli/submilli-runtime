// expect-error: expected `number`, got `number | null`
// A call in the condition invalidates the preceding module-variable guard.
let current: number | null = 3;
function clear(): boolean { current = null; return false; }
export function read(): number {
  if (current === null || clear()) { return 0; }
  return current;
}
export function relay(value: number): number { return value; }
export class Relay {
  constructor(public value: number) {}
  read(): number { return this.value; }
  relay(value: number): number { return value; }
}
