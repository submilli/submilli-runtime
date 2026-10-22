// JavaScript rejects a `"use strict"` directive in a function whose parameter
// list has a default, a pattern or a rest parameter (tsc: TS1346 at each such
// parameter, TS1347 at the directive). A directive anywhere in the prologue
// counts, and, as in tsc, so does an escaped spelling of one; a string after the
// prologue or a template is just an expression.
// expect-error-count: 14
// expect-error: this parameter is not allowed with a `"use strict"` directive
// expect-error: a `"use strict"` directive can't be used with a parameter list
function withDefault(a: number = 1): void {
  "use strict";
}
function laterInPrologue(a: number, ...rest: number[]): void {
  "other";
  'use strict';
}
const arrow = ({ x }: { x: number }): void => {
  "use strict";
};
class C {
  m(a: number, b: number = 2): void {
    "use strict";
  }
}
const expression = function (a: number = 1): void {
  "use strict";
};
const literal = {
  m(...rest: number[]): void {
    "use strict";
  },
};

function simple(a: number, b?: number): void {
  "use strict";
}
function afterPrologue(a: number = 1): void {
  const x = 1;
  "use strict";
}
function escaped(a: number = 1): void {
  "use\u0020strict";
}
function template(a: number = 1): void {
  `use strict`;
}

function main(): void {}
