export type J = number | null | W[];
export type W = J | string;

export function nullOf(): W {
  return null;
}

export function someOf(): W {
  return 4;
}

export function take(x: W): string {
  return JSON.stringify(x);
}
