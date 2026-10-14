let events = "";
class Yield {
  get done(): false { events += "done;"; return false; }
  get value(): [string, number] { events += "value;"; return ["answer", 42]; }
}
class Cursor {
  private count: number = 0;
  get next(): () => IteratorResult<[string, number]> {
    events += "next;";
    return (): IteratorResult<[string, number]> => {
      this.count += 1;
      if (this.count === 1) return new Yield();
      return { done: true, value: undefined };
    };
  }
  get close(): () => void { events += "close-get;"; return (): void => { events += "close;"; }; }
}
class Source {
  get iterator(): () => Iterator<[string, number]> {
    events += "iterator;";
    return (): Iterator<[string, number]> => new Cursor();
  }
}
class ThrowingValue {
  done: false = false;
  get value(): [string, number] { throw new Error("value failed"); }
}
function main(): void {
  const iterable: Iterable<[string, number]> = new Source();
  const result = Object.fromEntries(iterable);
  assert(result.answer === 42);
  assert(events === "iterator;next;done;value;");
  let closed = false;
  const broken: Iterable<[string, number]> = {
    iterator: (): Iterator<[string, number]> => ({
      next: (): IteratorResult<[string, number]> => new ThrowingValue(),
      close: (): void => { closed = true; },
    }),
  };
  let message = "";
  try { Object.fromEntries(broken); } catch (e: Error) { message = e.message; }
  assert(message === "value failed");
  assert(closed);
  const pairs: [string, number][] = [["set", 7]];
  assert(Object.fromEntries(new Set<[string, number]>(pairs)).set === 7);
}
