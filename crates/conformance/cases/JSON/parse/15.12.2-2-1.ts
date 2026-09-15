// test262: test/built-ins/JSON/parse/15.12.2-2-1.js

function main(): void {
  for (let i = 0; i <= 31; i++) {
    const name: string = String.fromCharCode(i);
    assertThrows((): void => {
      const o: { x?: number } = JSON.parse("{ \"" + name + "\" : \"John\" } ") as { x?: number };
      assertSameValue(o.x, o.x, "unreachable");
    }, "raw control character U+" + i.toString(16) + " in a property name is a syntax error");
  }
}
