// An enum imported under another name is the same enum, not a new type.
import { E as G, E } from "./m_enum";

function same(g: G, e: E): boolean {
  return g === e;
}

function takeG(g: G): number {
  return g;
}

function onlyAlias(g: G): boolean {
  return g === G.A;
}

/** Uses the enum through both names. */
export function run(): string {
  return `${same(G.A, E.A)} ${takeG(E.A)} ${onlyAlias(E.B)}`;
}

import { S as T, S } from "./m_enum";

function sameS(t: T, s: S): boolean {
  return t === s;
}

function label(g: G): string {
  switch (g) {
    case E.A:
      return "a";
    case G.B:
      return "b";
  }
}

/** Uses the string enum through both names. */
export function runS(): string {
  const t: T = S.X;
  return `${sameS(t, S.Y)} ${label(E.A)}`;
}
