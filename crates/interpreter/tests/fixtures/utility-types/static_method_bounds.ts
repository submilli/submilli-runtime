// A static method's type-parameter bound may name its own class.
class User {
  name: string = "n";
  age: number = 1;
  static key<K extends keyof User>(key: K): K { return key; }
}

function main(): void {
  assert(User.key("age") === "age", "a bound over the declaring class");
}
