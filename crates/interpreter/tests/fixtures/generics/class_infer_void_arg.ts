class Box<T> { constructor(public value: T) {} }
function nothing(): void {}
function main(): void {
  const box = new Box(nothing());
  assert(box.value === undefined, "constructor infers void from an undefined-valued call");
}
