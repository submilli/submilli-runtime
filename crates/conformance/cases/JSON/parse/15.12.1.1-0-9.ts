// test262: test/built-ins/JSON/parse/15.12.1.1-0-9.js

interface Inner {
  x?: number;
}

type Elem = boolean | number | null;

interface Outer {
  property: Inner;
  prop2: Elem[];
}

function main(): void {
  const o: Outer = JSON.parse("\t\r \n{\t\r \n" +
    "\"property\"\t\r \n:\t\r \n{\t\r \n}\t\r \n,\t\r \n" +
    "\"prop2\"\t\r \n:\t\r \n" +
    "[\t\r \ntrue\t\r \n,\t\r \nnull\t\r \n,123.456\t\r \n]" +
    "\t\r \n}\t\r \n") as Outer;
  assertSameValue(o.prop2.length, 3, "prop2 length");
  assertSameValue(o.prop2[0], true, "prop2[0]");
  assertSameValue(o.prop2[1], null, "prop2[1]");
  assertSameValue(o.prop2[2], 123.456, "prop2[2]");
}
