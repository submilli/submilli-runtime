// test262: test/built-ins/JSON/parse/15.12.1.1-g6-2.js

function main(): void {
  const s: string = JSON.parse("\"\\\\\"") as string;
  assertSameValue(s, "\\", "'\\' is a valid JSONEscapeCharacter");
}
