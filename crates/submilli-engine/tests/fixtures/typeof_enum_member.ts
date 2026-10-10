// `typeof E.A` is the type of the member's value: Submilli types an enum member
// as its enum, so it is `E` (tsc's member literal type is narrower). `keyof
// unknown` names no keys, so it is `never`.
enum Level {
  Low,
  High,
}

enum Mode {
  Read = "read",
  Write = "write",
}

type NoKeys = keyof unknown;

function describe(level: typeof Level.Low, mode: typeof Mode.Write): string {
  return `${level}:${mode}`;
}

function neverCalled(key: NoKeys): string {
  return key;
}

function main(): void {
  console.log(describe(Level.Low, Mode.Write));
}
