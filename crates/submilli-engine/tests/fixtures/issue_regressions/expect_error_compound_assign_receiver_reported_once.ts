// expect-error: unreachable code
// expect-error: no fallthrough
// expect-error: cannot assign to field `x` of `Holder | undefined`: the receiver can be `undefined`
// expect-error-count: 3
// A compound assignment's receiver is also part of its lowered value; each
// defect inside it is still one diagnostic.
class Holder {
  x: number = 0;
}

function bump(h: Holder): void {
  (() => {
    return h;
    console.log("after return");
  })().x += 1;
  (() => {
    switch (h.x) {
      case 1:
        console.log("one");
      case 2:
        break;
    }
    return h;
  })().x += 1;
  (() => {
    if (h.x > 0) {
      return h;
    }
  })().x += 1;
}

function main(): void {
  bump(new Holder());
}
