// expect-error: arrow function does not return a value on all paths
// expect-error: function `inner` does not return a value on all paths
// expect-error: unreachable code
// expect-error: no fallthrough
// expect-error-count: 8
class Machine {
  private state: number = 0;
  // Fallthrough (1) and unreachable code (2) in a field-initializer closure.
  readonly step: (code: number) => void = (code: number): void => {
    switch (code) {
      case 1:
        this.state = 1;
      case 2:
        break;
    }
    return;
    this.state = 2;
  };

  constructor() {
    // Fallthrough in a constructor (3).
    switch (this.state) {
      case 0:
        this.state = 1;
      default:
        break;
    }
  }

  run(flag: boolean): number {
    // Missing return in an arrow (4) and a nested function (5) in a method.
    const pick = (): number => {
      if (flag) {
        return 1;
      }
    };
    function inner(): number {
      if (flag) {
        return 2;
      }
    }
    // Unreachable code in a closure in a method (6).
    const log = (): void => {
      return;
      console.log("after return");
    };
    log();
    return pick() + inner();
  }

  get label(): string {
    // Fallthrough in a getter (7).
    switch (this.state) {
      case 0:
        this.state = 1;
      case 1:
        return "one";
    }
    return "other";
  }

  set level(value: number) {
    // Fallthrough in a setter (8).
    switch (value) {
      case 0:
        this.state = 0;
      default:
        this.state = value;
    }
  }
}

function main(): void {
  console.log(new Machine().run(true).toString());
}
