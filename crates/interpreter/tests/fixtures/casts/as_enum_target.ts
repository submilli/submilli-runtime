// `as` to an enum checks at runtime that the value is one of its members.
enum E { A, B = 5 }
enum S { X = "x", Y = "y" }

function pick(raw: unknown): E {
  return raw as E;
}

function rejects(cast: () => void): string {
  try {
    cast();
    return "<none>";
  } catch (e: Error) {
    return e.message;
  }
}

function main(): void {
  assert(pick(0) === E.A, "0 is E.A");
  assert(pick(5) === E.B, "5 is E.B");
  const n: number = 5;
  assert((n as E) === E.B, "a number narrows to the enum");
  assert(("y" as unknown as S) === S.Y, "a string enum checks its values");

  const nested = { k: "x", list: [0, 5] } as unknown as { k: S; list: E[] };
  assert(nested.k === S.X && nested.list[1] === E.B, "enums inside shapes are checked");
  const maybe = (null as unknown) as E | null;
  assert(maybe === null, "null passes `E | null`");

  assert(rejects(() => { pick(2); }) === "type mismatch: expected E, got number", "2 is no member");
  assert(rejects(() => { pick("0"); }) !== "<none>", "a string is no number enum member");
  assert(rejects(() => { "z" as unknown as S; }) !== "<none>", "z is no member of S");
  assert(rejects(() => { [0, 1] as unknown as E[]; }) !== "<none>", "1 is no member of E");
}
