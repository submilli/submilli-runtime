interface Loop<T> { next: Loop<T[]>; get(): T }
function inspect(value: Loop<void>): void {}
function main(): void {
  const optional: { value?: Loop<void> } = {};
  assert(optional.value === undefined, "recursive void instantiation has a valid value type");
}
