// A failed cast inside a record or an index signature names the entry's key,
// not its position among the record's values.
interface Item {
  id: string;
}

interface Catalog {
  owner: string;
  [sku: string]: Item | string;
}

type Tree = { name: string; children: Record<string, Tree> };

class Counters {
  private label: string = "hidden";
  hits: number = 1;
  get misses(): string {
    return "none";
  }
}

function expectMessage(thunk: () => void): string {
  try {
    thunk();
  } catch (e: Error) {
    return e.message;
  }
  assert(false, "expected the cast to throw");
  return "";
}

function main(): void {
  const record = expectMessage((): void => {
    const r = JSON.parse("{\"a\":1,\"b\":\"x\"}") as Record<string, number>;
  });
  assert(record === "type mismatch: expected { [key: string]: number }, got object at $[\"b\"] (expected number, got string)", record);

  const indexed = expectMessage((): void => {
    const r = JSON.parse("{\"a\":{\"id\":1}}") as { [k: string]: Item };
  });
  assert(indexed === "type mismatch: expected { [key: string]: Item }, got object at $[\"a\"][\"id\"] (expected string, got number)", indexed);

  const quoted = expectMessage((): void => {
    const r = JSON.parse("{\"a\\\"b\":\"x\"}") as Record<string, number>;
  });
  assert(quoted === "type mismatch: expected { [key: string]: number }, got object at $[\"a\\\"b\"] (expected number, got string)", quoted);

  const empty = expectMessage((): void => {
    const r = JSON.parse("{\"\":true}") as Record<string, number>;
  });
  assert(empty === "type mismatch: expected { [key: string]: number }, got object at $[\"\"] (expected number, got boolean)", empty);

  const declaredAndIndexed = expectMessage((): void => {
    const r = JSON.parse("{\"owner\":\"o\",\"x1\":{\"id\":\"a\"},\"x2\":{\"id\":2}}") as Catalog;
  });
  assert(
    declaredAndIndexed === "type mismatch: expected Catalog, got object at $[\"x2\"][\"id\"] (expected string, got number)",
    declaredAndIndexed
  );

  const nested = expectMessage((): void => {
    const r = JSON.parse("{\"o1\":{\"j1\":\"s\"},\"o2\":{\"j1\":1,\"j2\":true}}") as Record<string, Record<string, number | string>>;
  });
  assert(
    nested === "type mismatch: expected { [key: string]: { [key: string]: number | string } }, got object at $[\"o2\"][\"j2\"] (expected number | string, got boolean)",
    nested
  );

  // Every entry fails one alternative before another accepts it, and each
  // record names its own keys.
  const perRecord = expectMessage((): void => {
    const r = JSON.parse("[{\"a\":1,\"b\":\"s\"},{\"c\":2,\"d\":[{\"id\":3}]}]") as Record<string, number | string | Item[]>[];
  });
  assert(
    perRecord === "type mismatch: expected { [key: string]: number | string | Item[] }[], got array at $[1][\"d\"][0][\"id\"] (expected string, got number)",
    perRecord
  );

  const inArray = expectMessage((): void => {
    const r = JSON.parse("[{\"a\":1},{\"a\":1,\"z\":\"q\"}]") as Record<string, number>[];
  });
  assert(inArray === "type mismatch: expected { [key: string]: number }[], got array at $[1][\"z\"] (expected number, got string)", inArray);

  const recursive = expectMessage((): void => {
    const r = JSON.parse(
      "{\"name\":\"r\",\"children\":{\"x\":{\"name\":\"x\",\"children\":{\"y\":{\"name\":5,\"children\":{}}}}}}"
    ) as Tree;
  });
  assert(
    recursive === "type mismatch: expected Tree, got object at $[\"children\"][\"x\"][\"children\"][\"y\"][\"name\"] (expected string, got number)",
    recursive
  );

  // Private fields and absent optional fields have no entry, and a getter's
  // entry is named without its accessor prefix.
  const getter = expectMessage((): void => {
    const r = (new Counters() as unknown) as Record<string, number>;
  });
  assert(getter === "type mismatch: expected { [key: string]: number }, got object at $[\"misses\"] (expected number, got string)", getter);

  const optional: { a?: number; b: number | string } = { b: "s" };
  const absent = expectMessage((): void => {
    const r = (optional as unknown) as Record<string, number>;
  });
  assert(absent === "type mismatch: expected { [key: string]: number }, got object at $[\"b\"] (expected number, got string)", absent);
}
