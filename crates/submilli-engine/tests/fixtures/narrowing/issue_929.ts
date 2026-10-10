class Box<T> {
  value: T | number | null = null;
  target(value: T | number | null): void { this.value = value; }
  source(value: T | number): void { this.target(value); }
  nullable(value: T | null): void { this.target(value); }
}
class Child<T> extends Box<T> {
  forward(value: T | number): void { this.target(value); }
}
export function main(): void {
  const b = new Box<string>();
  b.source("yes");
  assert(b.value === "yes", "narrower generic union");
  b.source(3);
  assert(b.value === 3, "concrete union member");
  b.nullable(null);
  assert(b.value === null, "nullable widening");
  const c = new Child<string>();
  c.forward("child");
  assert(c.value === "child", "inherited receiver binding");
}
