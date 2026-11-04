import { load, save } from "@test/store";

interface Progress {
  step: number;
}

function main(): void {
  save("p", { step: 2 });
  const progress = load<Progress>("p");
  assert(progress !== undefined && progress.step === 2, "a conforming value is the checked Progress");
  assert(load<Progress>("missing") === undefined, "a missing key is undefined");

  save("wrong", { step: "two" });
  let message = "";
  try {
    load<Progress>("wrong");
  } catch (e: TypeError) {
    message = e.message;
  }
  assert(message.indexOf("does not match `Progress`") >= 0, "a mismatch names the caller's type");
}
