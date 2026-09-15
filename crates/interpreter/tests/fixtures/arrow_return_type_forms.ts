// Arrow functions with an explicit return-type annotation, one per form the
// type grammar accepts. The arrow disambiguator scans past the annotation with
// a separate lookahead (scan_past_type_annotation), so each form here pins
// that scanner against the real type parser.

interface Point {
  x: number;
  y: number;
}

function main(): string {
  const pair = (): [number, number] => [1, 2];
  const t = pair();
  assert(t[0] === 1);
  assert(t[1] === 2);

  const nested = (): [number, [string, boolean]] => [3, ["a", true]];
  assert(nested()[1][0] === "a");

  const tupleArray = (): [number, number][] => [[1, 2], [3, 4]];
  assert(tupleArray()[1][0] === 3);

  const named = (): number => 42;
  assert(named() === 42);

  const generic = (): Map<string, number> => new Map<string, number>();
  assert(generic().size === 0);

  const arr = (): number[] => [1, 2, 3];
  assert(arr().length === 3);

  const nullable = (): Point | null => null;
  assert(nullable() === null);

  const obj = (): { a: number } => ({ a: 7 });
  assert(obj().a === 7);

  const strLit = (): "on" | "off" => "on";
  assert(strLit() === "on");

  const numLit = (): 1 | 2 => 2;
  assert(numLit() === 2);

  const voidArrow = (): void => {};
  voidArrow();

  const higher = (): (n: number) => number => (n: number): number => n + 1;
  assert(higher()(41) === 42);

  const predicate = (v: unknown): v is string => typeof v === "string";
  assert(predicate("s"));
  assert(!predicate(5));

  // A parenthesized single name. `(T)` is spelled like v1's rejected bare parameter
  // list, and the `=>` that would tell them apart is the body's here — so in return
  // position the parens always group.
  const grouped = (): (number) => 9;
  assert(grouped() === 9);

  const groupedTwice = (): ((number)) => 10;
  assert(groupedTwice() === 10);

  const groupedNominal = (): (Point) => ({ x: 1, y: 2 });
  assert(groupedNominal().x === 1);

  const groupedArray = (): (number)[] => [11];
  assert(groupedArray()[0] === 11);

  const groupedUnion = (): (number) | null => 12;
  assert(groupedUnion() === 12);

  const groupedPredicate = (v: unknown): v is (string) => typeof v === "string";
  assert(groupedPredicate("s"));

  const groupedRight = (): null | (number) => 13;
  assert(groupedRight() === 13);

  const groupedGeneric = (): (Map<string, number>) => new Map<string, number>();
  assert(groupedGeneric().size === 0);

  const groupedNull = (): (null) => null;
  assert(groupedNull() === null);

  const groupedLiteral = (): ("on") => "on";
  assert(groupedLiteral() === "on");

  const groupedTuple = (): ([number, number]) => [14, 15];
  assert(groupedTuple()[1] === 15);

  const groupedObject = (): ({ a: number }) => ({ a: 16 });
  assert(groupedObject().a === 16);

  const groupedWithParams = (x: number, y: number): (number) => x + y;
  assert(groupedWithParams(8, 9) === 17);

  const groupedBlockBody = (): (number) => {
    return 18;
  };
  assert(groupedBlockBody() === 18);

  const groupedNestedArrow = (): ((v: number) => (number)) =>
    (v: number): (number) => v + 1;
  assert(groupedNestedArrow()(18) === 19);

  const groupedInTemplate = `${((): (number) => 20)()}`;
  assert(groupedInTemplate === "20");

  return "ok";
}
