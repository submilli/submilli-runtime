function main(): void {
  let marker: string = "before";
  const value = { toJson: (): string => "{\"marker\":\"" + marker + "\"}" };
  const space = (): number => {
    marker = "after";
    return 2;
  };

  const json = JSON.stringify(value, null, space());
  assert(json === "{\n  \"marker\": \"after\"\n}", "stringify evaluates space before toJson");
}
