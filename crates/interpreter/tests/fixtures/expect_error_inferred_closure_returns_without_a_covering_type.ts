// expect-error-count: 1
// expect-error: return type `Loose` conflicts with earlier return `Narrow`
// `Wide` fits `Narrow`, but neither fits `Loose`: `Loose`'s fields are all
// optional and `Narrow` shares none of them, and `Wide`'s `q` has another
// type. No return type covers all three, so the closure needs an annotation.

type Wide = { p: number; q: number };
type Narrow = { p: number };
type Loose = { q?: string };

function wide(): Wide {
  return { p: 1, q: 2 };
}
function narrow(): Narrow {
  return { p: 1 };
}
function loose(): Loose {
  return {};
}

function main(): void {
  const key: number = [0][0];
  const pick = () => {
    if (key === 0) {
      return wide();
    }
    if (key === 1) {
      return narrow();
    }
    return loose();
  };
  pick();
}
