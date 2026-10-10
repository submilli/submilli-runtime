export interface Shape {
  kind: string;
  size: number;
}

export function makeShape(size: number): Shape {
  return { kind: "box", size };
}

export const type = 7;
