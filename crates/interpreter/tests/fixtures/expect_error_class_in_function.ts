// expect-error: a `class` must be declared at the top level of the module
// expect-error: a `class` must be declared at the top level of the module
// expect-error: a `class` must be declared at the top level of the module
// expect-error-count: 3
// Classes are collected from the module's top level only. One declared in a
// function, a method or a block is reported, rather than dropped with its
// body unchecked.
function outer(): void {
  class Local {
    m(): number {
      return 1;
    }
  }
}

class Holder {
  method(): void {
    class InMethod {}
  }
}

function main(): void {
  if (true) {
    class InBlock {}
  }
}
