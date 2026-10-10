enum FirstNumber { A = 1, B = 2 }
enum OtherNumber { Same = 1, Different = 3 }
enum FirstString { A = "a", B = "b" }
enum OtherString { Same = "a", Different = "c" }
class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
class Numbers extends Parent { value: FirstNumber = FirstNumber.A; }
class Strings extends Parent { value: FirstString = FirstString.A; }
function rejects(read: () => void): void {
  let caught = false;
  try { read(); } catch (e) { caught = e instanceof TypeError; }
  assert(caught, "nonmember must throw TypeError");
}
export function main(): void {
  const n = new Numbers();
  assert(n.value === FirstNumber.A, "numeric enum member");
  n.reset(OtherNumber.Same);
  assert(n.value === FirstNumber.A, "overlapping runtime values remain indistinguishable");
  n.reset(OtherNumber.Different);
  rejects(() => { const value = n.value; });
  const s = new Strings();
  assert(s.value === FirstString.A, "string enum member");
  s.reset(OtherString.Same);
  assert(s.value === FirstString.A, "overlapping string enum value");
  s.reset(OtherString.Different);
  rejects(() => { const value = s.value; });
}
