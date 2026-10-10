import { Level } from "@test/levels";
import { describe } from "@test/consumer";

function own(level: Level): string {
  switch (level) {
    case Level.Low:
      return "script low";
    case Level.High:
      return "script high";
  }
}

function main(): void {
  assert(describe(Level.High) === "high", "the package's switch returns");
  assert(own(Level.Low) === "script low", "the script's switch returns");
}
