type A = { kind: "a"; x: number };
type B = { kind: "b"; y: number };
function one<T extends A>(value: T): number { return value.x; }
function many<T extends A | B>(values: T[]): number { return values.length; }
function st<T extends { status: "ok" | "err" }>(value: T): string { return value.status; }
function keep<T extends { status: "ok" | "err" }>(value: T): T { return value; }
function wide<T extends { kind: string }>(value: T): T { return value; }

function main(): void {
  assert(one({ kind: "a", x: 1 }) === 1, "literal field in object constraint");
  assert(many([{ kind: "a", x: 1 }, { kind: "b", y: 2 }]) === 2, "discriminated union constraint in array");
  assert(st({ status: "ok", extra: 1 }) === "ok", "literal preservation allows extra fields");
  const value = keep({ status: "ok", extra: 1 });
  let copied = value.status;
  const exact: "ok" = copied;
  value.extra = 2;
  assert(exact === "ok" && value.extra === 2, "literal constraint does not freeze other fields");
  const mutable = wide({ kind: "a" });
  mutable.kind = "b";
  assert(mutable.kind === "b", "plain string fields still widen");
}
