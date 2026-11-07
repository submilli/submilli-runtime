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
  assert("p" in pick(), "the selected return keeps its fields");
}
