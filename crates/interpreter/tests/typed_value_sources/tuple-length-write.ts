function main(): number {
  const named: [number, number?] = [5, 6];
  const keyed: [number, number?] = [7, 8];
  named.length = 1;
  const resize = (tuple: [number, number?], size: 1 | 2): void => {
    tuple["length"] = size;
  };
  resize(keyed, 1);
  return named.length === 1 && keyed.length === 1 ? 0 : 1;
}
