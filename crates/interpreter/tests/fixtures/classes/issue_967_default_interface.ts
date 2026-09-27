interface Zero { m(): number }
interface One { m(n: number): number }
class Defaults implements Zero, One {
  m(n: number = 3, extra: number = 4): number { return n + extra; }
}
class Override extends Defaults { m(n: number = 10, extra: number = 20): number { return n + extra; } }
function zero(value: Zero): number { return value.m(); }
function one(value: One): number { return value.m(5); }
function main(): void {
  assert(zero(new Defaults()) === 7, "all defaults");
  assert(one(new Defaults()) === 9, "trailing default");
  assert(zero(new Override()) === 30, "override defaults");
  assert(one(new Override()) === 25, "override trailing default");
}
