// A function's body is not inside the loop around its declaration, so neither
// a nested function nor an arrow can `break` or `continue` it.
// expect-error: `continue` outside of a loop
// expect-error-count: 4
function main(): void {
  for (let i = 0; i < 3; i++) {
    function f(): void {
      if (i === 1) continue;
    }
    const g = (): void => {
      if (i === 2) break;
    };
    f();
    g();
    [1].forEach((x) => {
      if (x > 0) continue;
    });
    switch (i) {
      case 0: {
        const h = (): void => {
          break;
        };
        h();
        break;
      }
    }
  }
}
