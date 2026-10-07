// `Object.keys`, `values` and `entries` on an Error instance leave out the
// inherited `message`, and `name` unless the instance assigned it: in
// JavaScript neither is an own enumerable property. JSON agrees.
class Coded extends Error {
  code: number = 3;
}
class Named extends Error {
  constructor(message: string) {
    super(message);
    this.name = "Custom";
  }
}

function main(): void {
  const coded = new Coded("m");
  assert(JSON.stringify(Object.keys(coded)) === '["code"]', "keys");
  assert(JSON.stringify(Object.values(coded)) === "[3]", "values");
  assert(JSON.stringify(Object.entries(coded)) === '[["code",3]]', "entries");
  assert(JSON.stringify(Object.keys(coded)) === JSON.stringify(Object.keys(JSON.parse(JSON.stringify(coded)) as Record<string, unknown>)), "keys agree with JSON");
  assert(JSON.stringify(Object.keys(new Named("m"))) === '["name"]', "an assigned name is listed");
  assert(Object.keys(new Error("m")).length === 0, "a plain Error has no enumerable keys");
  console.log(JSON.stringify(Object.keys(coded)), JSON.stringify(Object.keys(new Named("m"))));
}
