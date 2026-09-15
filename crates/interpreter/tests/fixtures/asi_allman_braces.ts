// A brace on its own line after a clause keyword or a declaration header — the C/C#/Java
// style LLMs emit when not mimicking prettier. The clause keyword or header is not a
// statement, so ASI must not terminate one there.
function main(): void {
  let trace = "";

  try
  {
    trace = trace + "t";
    throw new Error("boom");
  }
  catch (e)
  {
    trace = trace + "c";
  }
  finally
  {
    trace = trace + "f";
  }
  assert(trace === "tcf", "`try` / `catch` / `finally` braces on their own lines");

  let n = 0;
  if (n === 0)
  {
    n = 1;
  }
  else
  {
    n = 2;
  }
  assert(n === 1, "`if` / `else` braces on their own lines");

  if (n === 9)
  {
    n = 8;
  }
  else
  if (n === 1)
  {
    n = 3;
  }
  assert(n === 3, "`else` followed by `if` on the next line");

  let spins = 0;
  do
  {
    spins = spins + 1;
  }
  while (spins < 2)
  assert(spins === 2, "`do` brace on its own line, and no `;` after the condition");

  while (spins < 3)
  {
    spins = spins + 1;
  }
  assert(spins === 3, "`while` brace on its own line");

  for (let i = 0; i < 2; i = i + 1)
  {
    spins = spins + 1;
  }
  assert(spins === 5, "`for` brace on its own line");

  for (const v of [1, 2])
  {
    spins = spins + v;
  }
  assert(spins === 8, "`for-of` brace on its own line");

  switch (spins)
  {
    case 8:
    {
      spins = spins + 1;
      break;
    }
    default:
    {
      spins = 0;
      break;
    }
  }
  assert(spins === 9, "`switch` and its `case` blocks Allman");

  {
    spins = spins + 1;
  }
  assert(spins === 10, "a bare block on its own lines");

  const arrow = (v: number): number =>
  {
    return v * 2;
  };
  assert(arrow(5) === 10, "arrow block body on the line after `=>`");

  if (
    spins === 10 &&
    trace === "tcf"
  )
  {
    spins = spins + 1;
  }
  assert(spins === 11, "a header split over several lines, then an Allman brace");

  if (spins === 11) // a trailing comment before the brace
  {
    spins = spins + 1;
  }
  assert(spins === 12, "a comment between the header and its brace");

  const impl = new Impl();
  assert(impl.tag() === "impl", "class header brace on its own line");
  assert(impl.doubled === 2, "accessor with an Allman brace");
  assert(Impl.make().v === 1, "static method with an Allman brace");
  assert(shape({ v: 7 }) === 7, "interface header brace on its own line");
  assert(pick(Color.Green) === 1, "enum header brace on its own line");
  assert(boxed(4) === 4, "generic function header, then an Allman brace");
  assert(new Box<number>(3).get() === 3, "generic class and interface headers Allman");
  assert(wrapped(2) === 2, "a wrapped union return type, then an Allman brace");
}

interface Holder<T>
{
  get(): T;
}

class Box<T>
  implements Holder<T>
{
  private readonly v: T;

  constructor(v: T)
  {
    this.v = v;
  }

  get(): T
  {
    return this.v;
  }
}

function boxed<T>(v: T): T
{
  return new Box<T>(v).get();
}

function wrapped(
  n: number,
): 
  | number
  | null
{
  return n;
}

interface Shaped
{
  v: number;
}

enum Color
{
  Red,
  Green
}

class Base
{
  tag(): string
  {
    return "base";
  }
}

class Impl
  extends Base
  implements Shaped
{
  v: number = 1;

  get doubled(): number
  {
    return this.v * 2;
  }

  static make(): Impl
  {
    return new Impl();
  }

  tag(): string
  {
    return "impl";
  }
}

function shape(s: Shaped): number
{
  return s.v;
}

function pick(c: Color): number
{
  return c === Color.Green ? 1 : 0;
}
