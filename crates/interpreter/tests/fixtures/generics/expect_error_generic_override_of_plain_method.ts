// A generic method's slot takes its type arguments; overriding a method
// without type parameters by a generic one would run without them.
// expect-error: method `log` declares type parameters, but the method it overrides does not
class Base {
  log(value: unknown): void {}
}

class Child extends Base {
  log<T>(value: T): void {}
}

function main(): void {}
