// Method calls and interface property accesses on `Number`, `String`,
// `Boolean`, and `BigInt` receivers (locals, non-escaping parameters, exported
// parameters, bounded type parameters, optional chains, and structural user
// classes).

type BoxedNum = Number;

function paramNum(n: Number): string {
  return `${n.toFixed()}|${n.toFixed(2)}|${n.toString(16)}|${n.toPrecision(3)}|${n.valueOf()}`;
}

function paramStr(s: String): string {
  let chars = "";
  for (const ch of s) {
    chars += ch;
  }
  return `${s.length}|${s.toUpperCase()}|${s.concat("!", "?")}|${s.slice(1, 4)}|${s.valueOf()}|${chars}`;
}

function paramBool(b: Boolean): string {
  return `${b.toString()}|${b.valueOf()}`;
}

function paramBigInt(b: BigInt): string {
  return `${b.toString()}|${b.toString(16)}|${b.valueOf()}`;
}

export function exportedNum(n: Number): string {
  return `${n.toFixed(1)}:${n.valueOf()}`;
}

export function exportedStr(s: String): string {
  return `${s.length}:${s.trim()}:${s.concat("-ok")}`;
}

export function exportedBool(b: Boolean): string {
  return `${b.toString()}:${b.valueOf()}`;
}

export function exportedOptStr(s?: String): string {
  return `${s?.length}:${s?.toLowerCase()}`;
}

function genericNum<T extends Number>(x: T): string {
  return `${x.toFixed(1)}|${x.toString()}|${x.valueOf()}`;
}

function genericStr<T extends String>(x: T): string {
  return `${x.length}|${x.charAt(0)}|${x.concat("!")}|${x.valueOf()}`;
}

function genericBool<T extends Boolean>(x: T): string {
  return `${x.toString()}|${x.valueOf()}`;
}

function genericBigInt<T extends BigInt>(x: T): string {
  return `${x.toString(16)}|${x.valueOf()}`;
}

function makeNum(v: Number): Number {
  return v;
}

function makeStr(v: String): String {
  return v;
}

function makeBool(v: Boolean): Boolean {
  return v;
}

function makeBigInt(v: BigInt): BigInt {
  return v;
}

function maybeNum(present: boolean): Number | undefined {
  return present ? makeNum(4.5) : undefined;
}

function maybeStr(present: boolean): String | undefined {
  return present ? makeStr("Hi") : undefined;
}

class CustomBool {
  private readonly flag: boolean;

  constructor(flag: boolean) {
    this.flag = flag;
  }

  valueOf(): boolean {
    return this.flag;
  }

  toString(): string {
    return this.flag ? "yes" : "no";
  }

  toJson(): string {
    return this.flag ? "true" : "false";
  }
}

class Holder {
  val: Boolean | { flag: boolean };

  constructor(val: Boolean | { flag: boolean }) {
    this.val = val;
  }
}

class BoolHolder extends Holder {
  val: Boolean;

  constructor(val: Boolean) {
    super(val);
    this.val = val;
  }
}

function main(): void {
  // Locals typed as wrapper interfaces (both direct literal init and opaque return).
  const directN: Number = 25.5;
  assert(directN.toFixed(1) === "25.5", "direct local Number.toFixed(1)");
  const ln: Number = makeNum(25.5);
  assert(ln.toFixed() === "26", "local Number.toFixed default");
  assert(ln.toFixed(2) === "25.50", "local Number.toFixed(2)");
  assert(ln.toString() === "25.5", "local Number.toString()");
  assert((255 as BoxedNum).toString(16) === "ff", "aliased Number.toString(16)");
  assert(ln.valueOf() === 25.5, "local Number.valueOf()");

  const directS: String = "Hello";
  assert(directS.toUpperCase() === "HELLO", "direct local String.toUpperCase()");
  const ls: String = makeStr("Hello");
  assert(ls.length === 5, "local String.length");
  assert(ls.toLowerCase() === "hello", "local String.toLowerCase()");
  assert(ls.concat(", ", "world") === "Hello, world", "local String.concat variadic");
  assert(ls.valueOf() === "Hello", "local String.valueOf()");

  const directB: Boolean = false;
  assert(directB.toString() === "false", "direct local Boolean.toString()");
  const lb: Boolean = makeBool(false);
  assert(lb.toString() === "false", "local Boolean.toString()");
  assert(lb.valueOf() === false, "local Boolean.valueOf()");

  const lbi: BigInt = makeBigInt(255n);
  assert(lbi.toString() === "255", "local BigInt.toString()");
  assert(lbi.toString(16) === "ff", "local BigInt.toString(16)");
  assert(lbi.valueOf() === 255n, "local BigInt.valueOf()");

  // Non-escaping parameters.
  assert(paramNum(15) === "15|15.00|f|15.0|15", "parameter Number methods");
  assert(paramStr("Abcd") === "4|ABCD|Abcd!?|bcd|Abcd|Abcd", "parameter String methods");
  assert(paramBool(true) === "true|true", "parameter Boolean methods");
  assert(paramBigInt(31n) === "31|1f|31", "parameter BigInt methods");

  // Escaping / exported parameters.
  assert(exportedNum(3.25) === "3.3:3.25", "exported Number methods");
  assert(exportedStr("  hi ") === "5:hi:  hi -ok", "exported String methods");
  assert(exportedBool(false) === "false:false", "exported Boolean methods");
  assert(exportedOptStr("Hi") === "2:hi", "exported optional String present");
  assert(exportedOptStr() === "undefined:undefined", "exported optional String absent");

  // Bounded type parameters (`T extends Number`, `String`, `Boolean`, `BigInt`).
  assert(genericNum(7.25) === "7.3|7.25|7.25", "generic T extends Number with number");
  assert(genericNum(ln) === "25.5|25.5|25.5", "generic T extends Number with Number");
  assert(genericStr("xyz") === "3|x|xyz!|xyz", "generic T extends String with string");
  assert(genericStr(ls) === "5|H|Hello!|Hello", "generic T extends String with String");
  assert(genericBool(true) === "true|true", "generic T extends Boolean with boolean");
  assert(genericBool(lb) === "false|false", "generic T extends Boolean with Boolean");
  assert(genericBigInt(16n) === "10|16", "generic T extends BigInt with bigint");
  assert(genericBigInt(lbi) === "ff|255", "generic T extends BigInt with BigInt");

  // Optional chains on non-escaping wrapper-typed locals.
  const presentN = maybeNum(true);
  const absentN = maybeNum(false);
  assert(presentN?.toFixed(1) === "4.5", "optional Number?.toFixed present");
  assert(absentN?.toFixed(1) === undefined, "optional Number?.toFixed absent");

  const presentS = maybeStr(true);
  const absentS = maybeStr(false);
  assert(presentS?.length === 2, "optional String?.length present");
  assert(presentS?.toUpperCase() === "HI", "optional String?.toUpperCase present");
  assert(absentS?.length === undefined, "optional String?.length absent");
  assert(absentS?.toUpperCase() === undefined, "optional String?.toUpperCase absent");

  // Structural user class assignable to `Boolean` and `T extends Boolean`.
  const customFalse = new CustomBool(false);
  const customTrue = new CustomBool(true);
  const asBool: Boolean = customFalse;
  assert(asBool.toString() === "no", "structural class as Boolean.toString()");
  assert(asBool.valueOf() === false, "structural class as Boolean.valueOf()");
  assert(paramBool(customTrue) === "yes|true", "structural class passed to Boolean param");
  assert(exportedBool(customFalse) === "no:false", "structural class passed to exported Boolean param");
  assert(genericBool(customTrue) === "yes|true", "structural class passed to T extends Boolean");

  // Runtime interface guard on `Boolean` accepts primitives and conforming
  // structural objects, and rejects non-conforming objects with a TypeError.
  const bh = new BoolHolder(true);
  assert(bh.val.toString() === "true", "narrowed Boolean field holds primitive");
  const base: Holder = bh;
  base.val = customTrue;
  assert(bh.val.toString() === "yes", "narrowed Boolean field holds structural class");
  base.val = { flag: true };
  let threw = false;
  try {
    const bad = bh.val;
    assert(bad.valueOf() === false, "unreachable");
  } catch (e) {
    threw = e instanceof TypeError;
  }
  assert(threw, "non-conforming object in narrowed Boolean field throws TypeError");
}
