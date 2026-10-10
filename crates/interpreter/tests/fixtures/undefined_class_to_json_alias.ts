// Submilli's custom toJson protocol returns serialized JSON text. JavaScript's
// JSON.stringify does not invoke this method. Submilli also checks the string
// result of universal conversion slots. These contracts are Submilli-specific.
interface JsonConversion { value: number; toJson?(): string; }
class JsonBox implements JsonConversion {
  value: number = 2;
  toJson(): string { return "1"; }
}
class DerivedJsonBox extends JsonBox { extra: string = "field"; }
function replace(value: JsonConversion): void {
  value.toJson = function(this: JsonConversion): string { return String(this.value); };
}
function clear(value: JsonConversion): void { value.toJson = undefined; }
function replaceWithNumber(value: JsonConversion): void {
  value.toJson = (((): number => 1) as unknown) as () => string;
}
function expectStringTypeError(convert: () => unknown): void {
  let caught = false;
  try {
    convert();
  } catch (error) {
    assert(error instanceof TypeError, "invalid conversion result is catchable");
    assert(error.message === "type mismatch: expected string, got number");
    caught = true;
  }
  assert(caught, "the universal conversion slot checks its string result");
}
function check(value: JsonBox): void {
  replace(value);
  assert(value.toJson() === "2", "direct conversion reads the replacement");
  assert(JSON.stringify(value) === "2", "JSON conversion binds the replacement receiver");
  const erased: unknown = value;
  assert(JSON.stringify(erased) === "2", "erased JSON conversion reads the replacement");
  assert(JSON.stringify([value]) === "[2]", "nested JSON conversion reads the replacement");
  replaceWithNumber(value);
  expectStringTypeError((): unknown => JSON.stringify(value));
  clear(value);
  let caught = false;
  try {
    JSON.stringify(value);
  } catch (error) {
    assert(error instanceof TypeError);
    caught = true;
  }
  assert(caught, "clearing the conversion raises a catchable TypeError");
}
interface StringConversion { toString?(): string; }
class Printable { toString(): string { return "old"; } }
function checkInvalidStringResult(value: StringConversion): void {
  value.toString = (((): number => 1) as unknown) as () => string;
  expectStringTypeError((): unknown => String(value));
}
class Empty {}
function defaultJson(value: Empty | null | undefined): string | undefined {
  return value?.toJson?.();
}
function main(): void {
  check(new JsonBox());
  check(new DerivedJsonBox());
  checkInvalidStringResult(new Printable());
  assert(new Empty().toJson?.() === "{}", "default conversion has no mutable method payload");
  assert(defaultJson(new Empty()) === "{}");
  assert(defaultJson(null) === undefined && defaultJson(undefined) === undefined);
}
