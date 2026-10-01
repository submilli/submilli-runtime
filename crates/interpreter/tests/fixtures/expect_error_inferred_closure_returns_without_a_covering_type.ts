// expect-error-count: 1
// expect-error: cannot infer one return type for this closure; annotate its return type
// Each return's type covers the one before it (`Wide` fits `Narrow`, `Narrow`
// fits `Loose`), but none covers all three: `Wide` is not assignable to `Loose`,
// whose `q` has another type. The closure gets no type.

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
