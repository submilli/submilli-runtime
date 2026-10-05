// JavaScript scopes a `switch` body as one block: a function declared in one
// clause is hoisted to the body's start, so every clause can call it.

function describe(kind: number): string {
  switch (kind) {
    case 1:
      return label("one");
    case 2:
      function label(text: string): string {
        return "<" + text + ">";
      }
      return label("two");
    default:
      return label("other") + shout("!");
  }
  function shout(text: string): string {
    return text + text;
  }
}
assert(describe(1) === "<one>", "called in a clause before the one declaring it");
assert(describe(2) === "<two>", "called in its own clause");
assert(describe(3) === "<other>!!", "called in `default`");

function withLocal(kind: number): string {
  switch (kind) {
    case 1:
      const prefix = "p:";
      function tag(text: string): string {
        return prefix + text;
      }
      return tag("a");
    default:
      return "none";
  }
}
assert(withLocal(1) === "p:a", "a function using its clause's local, in that clause");
assert(withLocal(2) === "none", "the other clause doesn't run it");

function braced(kind: number): string {
  switch (kind) {
    case 1: {
      const x = "first";
      return x;
    }
    case 2: {
      const x = "second";
      return x;
    }
  }
  return "none";
}
assert(braced(1) + braced(2) === "firstsecond", "braced clauses keep their own scope");

function nested(outer: number, inner: number): string {
  switch (outer) {
    case 1:
      function pick(): string {
        return "outer";
      }
      switch (inner) {
        case 1:
          function pick(): string {
            return "inner";
          }
          return pick();
        default:
          return pick();
      }
    default:
      return pick();
  }
}
assert(nested(1, 1) + nested(1, 2) + nested(2, 0) === "innerinnerouter", "nested switches");

function code(): number {
  return 1;
}
// The discriminant runs before the switch body, so it calls the outer `code`.
function discriminantOutside(): string {
  switch (code()) {
    case 1:
      return "outer";
    default:
      function code(): number {
        return 2;
      }
      return "inner " + String(code());
  }
}
assert(discriminantOutside() === "outer", "the discriminant doesn't see a hoisted function");

// `default` sits between cases; each clause still sees every function.
function defaultInMiddle(kind: number): string {
  let out = "";
  for (let i = 0; i < 2; i++) {
    switch (kind + i) {
      case 1:
        out += even(2) ? "e" : "o";
        break;
      default:
        out += odd(3) ? "O" : "E";
        break;
      case 3:
        function even(n: number): boolean {
          return n === 0 ? true : odd(n - 1);
        }
        function odd(n: number): boolean {
          return n === 0 ? false : even(n - 1);
        }
        out += "3";
        break;
    }
  }
  return out;
}
assert(defaultInMiddle(1) + defaultInMiddle(2) === "eO" + "O3", "mutual recursion across clauses, in a loop");

let topLevel = "";
switch (topLevel.length) {
  case 0:
    topLevel = mark("top");
    break;
  default:
    function mark(text: string): string {
      return "[" + text + "]";
    }
}
assert(topLevel === "[top]", "a switch at the top level");

function main(): void {}
