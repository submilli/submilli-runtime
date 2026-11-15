function result(): { g: number } { return Object.assign({}, { g: 1 }); }
function generic<T>(value: T): { g: T } { return Object.assign({}, { g: value }); }
function main(): void {
  const r: { g: number } = Object.assign({}, { g: 2 });
  const s: { g: number } = Object.assign({ g: 0 }, { g: 3 });
  assert(result().g === 1 && r.g === 2 && s.g === 3);
  assert(generic("ok").g === "ok");
}
