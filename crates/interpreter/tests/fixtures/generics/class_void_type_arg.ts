class Box<T> { constructor(public value: T) {} }
function main(): void {
  const empty: Box<void> | null = null;
  assert(empty === null, "void class type argument is legal in annotations");
  const box: Box<void> = new Box<void>(undefined);
  assert(box.value === undefined, "explicit void type argument accepts undefined");
}
