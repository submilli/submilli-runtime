// SUB-2148: a missing read that reaches no check throws `MissingValueError`,
// its own class, not a `RangeError`. A check never reaches into a call, or
// past a field read of the missing value.
interface Item {
  name: string;
}

const someArray: number[] = [1];

function price(item: Item): number {
  return someArray[100];
}

function outcome(read: () => string): string {
  try {
    return "=" + read();
  } catch (e: RangeError) {
    return "RangeError";
  } catch (e) {
    return e.name + ": " + e.message;
  }
}

function main(): void {
  const a: Item[] = [{ name: "x" }];
  const r: Record<string, string> = {};
  const i = 5;
  const key = "k";

  assert(outcome(() => a[i]?.name ?? "d") === "=d", "a[i]?.name ?? d");
  assert(outcome(() => { const v = a[i]; return v?.name ?? "d"; }) === "=d", "through a local");
  assert(
    outcome(() => a[i].name) === "MissingValueError: index 5 is out of range for `a` (length 1); use `a[5] ?? …` or `a.at(5)`",
    "a field read of a missing element throws at the element",
  );
  assert(
    outcome(() => a.map((item) => r[item.name]).join()) === "MissingValueError: key \"x\" is missing from `r`; use `r[key] ?? …` or check `key in r` first",
    "a read in a callback",
  );
  assert(
    outcome(() => String(price(a[0]))) === "MissingValueError: index 100 is out of range for `someArray` (length 1); use `someArray[100] ?? …` or `someArray.at(100)`",
    "a check never reaches into a call",
  );
  assert(
    outcome(() => r[key]) === "MissingValueError: key \"k\" is missing from `r`; use `r[key] ?? …` or check `key in r` first",
    "a record read",
  );
  assert(
    outcome(() => r.absent) === "MissingValueError: key \"absent\" is missing from `r`; use `r[key] ?? …` or check `key in r` first",
    "a dotted record read",
  );
  assert(
    outcome(() => "ab"[7]) === "MissingValueError: index 7 is out of range (length 2); use `?? …` at the read, or `.at(7)`",
    "a string read",
  );
  const m = /(a)|(b)/.exec("b");
  if (m === null) throw new Error("no match");
  assert(
    outcome(() => m[1]) === "MissingValueError: group 1 of `m` did not take part in the match; use `m[1] ?? …`",
    "a regex group",
  );
  let caught = "";
  try {
    const value: string = r[key];
    caught = value;
  } catch (e) {
    caught = (e instanceof MissingValueError ? "missing" : "other") + (e instanceof RangeError ? " range" : "");
  }
  assert(caught === "missing", "MissingValueError is not a RangeError");
}
