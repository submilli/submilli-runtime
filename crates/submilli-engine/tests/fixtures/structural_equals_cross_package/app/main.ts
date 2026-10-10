import { makePoint, makeWide } from "@test/points";

function main(): void {
  const remote: unknown = makePoint(1, 2);
  const local: unknown = { x: 1, y: 2 };
  assert(remote === local);
  assert(local === remote);

  assert(makePoint(1, 2) === { x: 1, y: 2 });
  assert(makePoint(1, 2) !== { x: 1, y: 3 });

  const renamed: unknown = { x: 1, z: 2 };
  assert(remote !== renamed);
  assert(renamed !== remote);

  const wide: unknown = makeWide();
  assert(wide !== local);
  assert(local !== wide);
}
