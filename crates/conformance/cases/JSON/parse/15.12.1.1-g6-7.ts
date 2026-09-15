// test262: test/built-ins/JSON/parse/15.12.1.1-g6-7.js

function main(): void {
  const s: string = JSON.parse("\"\\t\"") as string;
  assertSameValue(s, "\t", "'t' is a valid JSONEscapeCharacter");
}
