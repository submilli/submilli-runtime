// An object literal right after a cast's `>` or a spread's `...` keeps its last
// field: automatic semicolon insertion must not treat its `}` as closing a block.
interface Point {
  x: number;
  y: number;
}

function origin(): Point {
  return <Point>{ x: 0, y: 0 }
}

function main(): void {
  const p = <Point>{
    x: 1,
    y: 2
  }
  assert(p.y === 2, "multi-line literal after a cast");
  assert(origin().x === 0, "cast after `return`, then a newline");

  const ps = [<Point>{ x: 3, y: 4 }, <Point>{ x: 5, y: 6 }];
  assert(ps[1].x === 5, "casts as array elements");

  const merged = {
    ...{ x: 7, y: 8 },
    y: 9
  }
  assert(merged.x === 7 && merged.y === 9, "object literal as a spread operand");

  let later = 0
  later = 1
  assert(later === 1, "statements after these still get their semicolons");
}
