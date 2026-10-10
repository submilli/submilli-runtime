// expect-error: type parameters on class instance methods are not supported yet
class Util {
  identity<U>(v: U): U {
    return v;
  }
}

function main(): void {}
