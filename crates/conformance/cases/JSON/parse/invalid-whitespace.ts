// test262: test/built-ins/JSON/parse/invalid-whitespace.js

function main(): void {
  const spaces: string[] = [
    "\u1680", "\u180e", "\u2000", "\u2001", "\u2002", "\u2003",
    "\u2004", "\u2005", "\u2006", "\u2007", "\u2008", "\u2009",
    "\u200a", "\u202f", "\u205f", "\u3000",
  ];
  for (let i = 0; i < spaces.length; i++) {
    const text: string = spaces[i] + "1";
    assertThrows((): void => {
      const n: number = JSON.parse(text) as number;
      assertSameValue(n, n, "unreachable");
    }, "category-Z space U+" + spaces[i].charCodeAt(0).toString(16) + " is not JSON whitespace");
  }
}
