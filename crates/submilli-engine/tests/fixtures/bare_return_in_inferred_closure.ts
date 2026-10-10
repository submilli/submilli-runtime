// A bare `return` counts as a `void` return when an unannotated closure's
// return type is inferred. Beside other `void` returns, a body that falls off
// its end, or a diverging `return`, the closure is still `void`.

function boom(): never {
  throw new Error("boom");
}

let calls = 0;
function touch(): void {
  calls = calls + 1;
}

function runVoid(f: () => void): void {
  f();
}

function main(): void {
  const key: string = ["a"][0];
  let fellThrough = 0;
  const bareThenFallOff = () => {
    if (key === "b") {
      return;
    }
    fellThrough = fellThrough + 1;
  };
  bareThenFallOff();
  assert(fellThrough === 1, "bare return then falling off the end");

  const bareThenVoidCall = () => {
    if (key === "b") {
      return;
    }
    return touch();
  };
  const voidCallThenBare = () => {
    if (key === "a") {
      return touch();
    }
    return;
  };
  bareThenVoidCall();
  voidCallThenBare();
  assert(calls === 2, "bare return beside a returned void call, in both orders");

  const neverThenBare = () => {
    if (key === "b") {
      return boom();
    }
    return;
  };
  const bareThenNever = () => {
    if (key === "a") {
      return;
    }
    return boom();
  };
  const expression = function () {
    if (key === "b") {
      return boom();
    }
    return;
  };
  neverThenBare();
  bareThenNever();
  expression();
  runVoid(neverThenBare);
  runVoid(bareThenNever);

  let threw = false;
  const bareThenThrow = () => {
    if (key === "b") {
      return;
    }
    throw new Error("thrown");
  };
  try {
    bareThenThrow();
  } catch (e) {
    threw = true;
  }
  assert(threw, "bare return beside a throw");

  // A value return after a diverging one still names the closure's type.
  const neverThenValue = () => {
    if (key === "b") {
      return boom();
    }
    return 7;
  };
  const seven: number = neverThenValue();
  assert(seven === 7, "value return after a never return");
}
