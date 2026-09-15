// A template substitution is an expression, so a newline inside `${ … }` is a wrap, not a
// statement end — prettier breaks long substitutions exactly this way.
function double(v: number): number {
  return v * 2;
}

function main(): void {
  const n = 1;
  const head = `v=${
    n
  }`;
  assert(head === "v=1", "newline inside a `TemplateHead` substitution");

  const middle = `a${n}b${
    n + 1
  }c${
    n + 2
  }d`;
  assert(middle === "a1b2c3d", "newlines inside `TemplateMiddle` substitutions");

  const nested = `outer=${
    `inner=${
      n
    }`
  }`;
  assert(nested === "outer=inner=1", "a template nested inside a wrapped substitution");

  const deep = `A${`B${`C${
    n
  }C`}B`}A`;
  assert(deep === "AB C1C B A".split(" ").join(""), "three templates deep");

  const call = `len=${double(
    2
  )}`;
  assert(call === "len=4", "a wrapped call inside a substitution");

  const chain = `n=${[1, 2, 3]
    .map(double)
    .length}`;
  assert(chain === "n=3", "a wrapped member chain inside a substitution");

  const block = `mapped=${
    [1, 2].map((v: number): number => {
      const doubled = v * 2
      return doubled
    }).length
  }`;
  assert(block === "mapped=2", "an arrow block body inside a substitution still gets ASI");

  const obj = `field=${
    {
      a: 5,
    }.a
  }`;
  assert(obj === "field=5", "an object literal opens a value list right after `${`");

  const arr = `count=${
    [
      1,
      2,
    ].length
  }`;
  assert(arr === "count=2", "an array literal wrapped inside a substitution");

  const cond = `pick=${
    n === 1
      ? "one"
      : "other"
  }`;
  assert(cond === "pick=one", "a wrapped ternary inside a substitution");

  const braces = `${"}"}${"${"}`;
  assert(braces === "}${", "braces inside a string inside a substitution");
}
