import { Level } from "@test/levels";

/**
 * Names a level.
 * @param level Level to describe.
 * @returns The level name.
 */
export function describe(level: Level): string {
  switch (level) {
    case Level.Low:
      return "low";
    case Level.High:
      return "high";
  }
}
