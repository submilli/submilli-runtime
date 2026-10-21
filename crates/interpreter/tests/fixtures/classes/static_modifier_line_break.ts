// As in TypeScript, `static` alone may end a line and still modify the member on
// the next (other modifiers can't: there the word is the member's name). A
// `static` that no member name follows is itself the name.
class Config {
  static
  retries: number = 3;
  static
  label(): string {
    return "config";
  }
  private static
  secret: number = 7;
  static
  readonly mode: string = "fast";
  static reveal(): number {
    return Config.secret;
  }
}

class Named {
  static: number = 1;
}

function main(): void {
  assert(Config.retries === 3, "a static field");
  assert(Config.label() === "config", "a static method");
  assert(Config.reveal() === 7, "a private static field");
  assert(Config.mode === "fast", "a static readonly field");
  assert(new Named().static === 1, "a field named static");
  console.log(Config.retries, Config.label(), Config.mode);
}
