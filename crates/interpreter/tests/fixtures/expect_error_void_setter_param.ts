// A setter's parameter is resolved by the class signature and body passes, both
// separate from `resolve_param`.
// expect-error: `void` cannot be a parameter type — it has no values
class Holder {
  private w: number = 0;
  set v(x: void) {
    this.w = 1;
  }
}

function main(): void {}
