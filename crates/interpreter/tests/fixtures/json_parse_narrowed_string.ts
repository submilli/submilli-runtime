// `JSON.parse` takes any argument assignable to `string`, as a call does: a
// `string` narrowed from `string | T` is `string | (T & string)`.
function parse<T>(data: string | T): unknown {
  if (typeof data === "string") {
    return JSON.parse(data);
  }
  return null;
}

function main(): void {
  const parsed = parse<number>("[1, 2]");
  assert(Array.isArray(parsed), "the narrowed string parses");
  assert(parse<number>(5) === null, "the other branch");
}
