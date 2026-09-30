import { Level } from "@test/levels";

/** Names a level. */
export function describe(level: Level): string {
  switch (level) {
    case Level.Low:
      return "low";
    case Level.High:
      return "high";
  }
}
