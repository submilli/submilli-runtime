// expect-warning: `??` on non-nullable type
function main(): void {
  nonnullable("x");
  alternatives(true, null);
  alternatives(false, "y");
  const values: Record<"x" | "y", number> = { x: 1, y: 10 };
  values["x"] = 2;
  assert(values.x === 2);
  values["x"] += 3;
  assert(values.x === 5);
  const before = values["x"]++;
  assert(before === 5 && values.x === 6);
  values["y"]--;
  assert(values.y === 9);
  values[("x")] = 7;
  assert(values[("x")] === 7);
  const computed: Record<"x", number> = { [("x")]: 8 };
  assert(computed.x === 8);
  assert(optional(computed) === 8 && optional(null) === undefined);
  const unknown: { x: unknown } = { x: null };
  unknown["x"] = "ok";
  assert(unknown.x === "ok");
}

function optional(values: Record<"x", number> | null): number | undefined { return values?.[("x")]; }

function alternatives(flag: boolean, key: "y" | null): void {
  const values: Record<"x" | "y", number> = { x: 1, y: 2 };
  values[flag ? "x" : "y"] = 3;
  assert(values[flag ? "x" : "y"] === 3);
  values[key ?? "x"] = 4;
  assert(values[key ?? "x"] === 4);
  values[key ?? "x"] += 1;
  const before = values[flag ? (key ?? "x") : "y"]++;
  assert(before === 5);
  assert(values[key ?? "x"] === 6);
}

function nonnullable(key: "x"): void {
  const values: Record<"x", number> = { x: 1 };
  values[key ?? "y"] = 3;
  assert(values[key ?? "y"] === 3);
}
