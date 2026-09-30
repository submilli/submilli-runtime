// expect-error-count: 0
import { Level } from "./levels";

// `shades.ts` declares another `Level` with a variant more; the first switch is
// over the one imported here.
enum Shade {
  Low,
  High,
  Extreme,
}

function describe(level: Level): string {
  switch (level) {
    case Level.Low:
      return "low";
    case Level.High:
      return "high";
  }
}

function shade(value: Shade): string {
  switch (value) {
    case Shade.Low:
      return "low";
    case Shade.High:
      return "high";
    case Shade.Extreme:
      return "extreme";
  }
}
