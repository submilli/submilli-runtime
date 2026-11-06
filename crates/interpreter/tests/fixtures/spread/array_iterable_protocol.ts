function collect<T>(source: Iterable<T>): T[] {
  return [...source];
}

function main(): void {
  const log: string[] = [];
  const values: Iterable<number> = {
    iterator: (): Iterator<number> => {
      log.push("iterator");
      let i = 0;
      return {
        next: (): IteratorResult<number> => {
          log.push("next");
          i += 1;
          if (i > 2) return { done: true, value: undefined };
          return { done: false, value: i };
        }
      };
    }
  };
  assert(collect(values).join(",") === "1,2", "generic Iterable");
  assert([...values].join(",") === "1,2", "fresh iterator on every spread");
  assert(log.join(",") === "iterator,next,next,next,iterator,next,next,next", "protocol ordering");
  const bad: Iterable<number> = {
    iterator: (): Iterator<number> => {
      return { next: (): IteratorResult<number> => { throw new Error("iteration failed"); } };
    }
  };
  let reached = false;
  let caught = false;
  try {
    const result = [...bad, (() => { reached = true; return 9; })()];
    assert(result.length === 0, "unreachable");
  } catch (e) {
    caught = e instanceof Error && e.message === "iteration failed";
  }
  assert(caught && !reached, "iterator errors propagate before the next element");
}
