// expect-error: unreachable code
// expect-error: arrow function does not return a value on all paths
// expect-error: no fallthrough
// expect-error-count: 4
class Machine {
  private state: number = 0;
  readonly peek: () => number = () => {
    if (this.state > 0) {
      return this.state;
    }
  };

  constructor() {
    return;
    this.state = 1;
  }

  set level(value: number) {
    throw new Error("read-only");
    this.state = value;
  }

  route(code: number): void {
    const handle = (): void => {
      switch (code) {
        case 1:
          this.state = 1;
        case 2:
          break;
      }
    };
    handle();
  }
}
