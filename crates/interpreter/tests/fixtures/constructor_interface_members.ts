interface Numeric {
  (value: string): number;
  MAX_SAFE_INTEGER: number;
  isInteger(value: unknown): boolean;
  absent?: string;
}
interface Textual {
  (value: number): string;
  fromCharCode(...codes: number[]): string;
}
interface NumericFields {
  (value: string): number;
  isInteger: (value: unknown) => boolean;
  parseInt: (text: string, radix?: number) => number;
}
interface TextualFields {
  (value: number): string;
  fromCharCode: (...codes: number[]) => string;
  fromCodePoint: (...points: number[]) => string;
}
interface FunctionMeta { (value: string): number; length?: number; }
let order = "";
function numbers(): Numeric {
  order += "receiver;";
  return Number;
}
function argument(): number {
  order += "argument;";
  return 3;
}
function inspect(number: Numeric): number {
  assert(number.isInteger(3));
  return number.MAX_SAFE_INTEGER;
}
function main(): void {
  // The structural view preserves the function's existing public length.
  // Native-constructor length metadata itself is tracked separately in SUB-1829.
  const numberMeta: FunctionMeta = Number;
  assert(typeof numberMeta.length === "number");
  const number: Numeric = Number;
  assert(number("3") === 3);
  assert(number.MAX_SAFE_INTEGER === 9007199254740991);
  assert(number.isInteger(3));
  assert(!number.isInteger(3.5));
  const numberFields: NumericFields = Number;
  const integer = numberFields.isInteger;
  assert(integer(3));
  assert(integer.length === 1);
  assert(numberFields.parseInt.length === 2);
  assert(numberFields.parseInt("10") === 10);
  assert(!integer("3"));
  assert(number.absent === undefined);
  assert(inspect(Number) === 9007199254740991);
  assert(numbers().MAX_SAFE_INTEGER === 9007199254740991);
  assert(order === "receiver;");
  order = "";
  assert(numbers().isInteger(argument()));
  assert(order === "receiver;argument;");
  assert(number?.isInteger(3));
  assert(numberFields.isInteger?.(3));
  const text: Textual = String;
  assert(text(42) === "42");
  assert(text.fromCharCode(72, 105) === "Hi");
  assert(text.fromCharCode() === "");
  const textFields: TextualFields = String;
  const chars = textFields.fromCharCode;
  assert(chars(72, 105) === "Hi");
  assert(chars.length === 1);
  assert(textFields.fromCodePoint.length === 1);
  assert(textFields.fromCodePoint(72, 105) === "Hi");
  assert(chars(0xd800).charCodeAt(0) === 0xd800);
  assert(text?.fromCharCode(72, 105) === "Hi");
  assert(text.fromCharCode(...[72, 105]) === "Hi");
}
