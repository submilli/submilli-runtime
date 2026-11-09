function hasKey(key: string | number, value: Record<string, boolean>): boolean {
  return key in value;
}

interface SeenIds { [id: string]: boolean; }
enum Id { Five = 5 }

function main(): void {
  const record: Record<string, boolean> = { "5": true };
  const id: number = 5;
  assert(id in record);
  assert(!(6 in record));
  const seen: SeenIds = {};
  seen[String(id)] = true;
  assert(id in seen);
  assert(!(6 in seen));
  const object = { "5": true, "1.5": false, "0": undefined };
  assert(5 in object);
  assert(1.5 in object);
  assert(-0 in object);
  assert(!(1 in object));
  assert(hasKey(5, record));
  assert(hasKey("5", record));
  assert(!hasKey(6, record));
  assert(Id.Five in record);
  const numbers: Record<number, boolean> = { "5": false };
  assert(id in numbers);
  const special: Record<string, boolean> = {
    "NaN": true, "Infinity": true, "-Infinity": true, "1e+21": true, "1e-7": true,
  };
  assert(NaN in special);
  assert(Infinity in special);
  assert(-Infinity in special);
  assert(1e21 in special);
  assert(1e-7 in special);
  let order = "";
  function key(): number { order += "k"; return 5; }
  function receiver(): Record<string, boolean> { order += "r"; return record; }
  assert(key() in receiver());
  assert(order === "kr");
}
