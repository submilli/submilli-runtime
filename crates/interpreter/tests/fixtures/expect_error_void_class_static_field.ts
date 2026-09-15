// expect-error: `void` cannot be a field type — it has no values
function nothing(): void {}

class Holder {
  static shared: void = nothing();
}

function main(): void {}
