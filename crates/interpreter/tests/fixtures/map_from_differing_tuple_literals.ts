// An array literal of differing tuples gives a generic constructor no single
// type argument, so the annotation's, which every tuple fits, stands.
enum AppType {
  Standard = "Standard",
  Relationship = "Relationship",
}
enum AppStyle {
  Tree,
  Standard,
  MiniApp,
}
type Key = "a" | "b";

function main(): void {
  const styles: Map<AppType, AppStyle[]> = new Map([
    [AppType.Standard, [AppStyle.Standard, AppStyle.MiniApp]],
    [AppType.Relationship, [AppStyle.Tree]],
  ]);
  const counts: Map<Key, number> = new Map([["a", 1], ["b", 2]]);
  styles.get(AppType.Relationship)?.push(AppStyle.Standard);
  console.log(styles.size, styles.get(AppType.Relationship)?.length, counts.get("b"));
  laterArgument();
}

function keysThen<K>(entries: [K, number][], first: K): K[] {
  const keys: K[] = [first];
  for (const entry of entries) {
    keys.push(entry[0]);
  }
  return keys;
}

function laterArgument(): void {
  const keys: AppType[] = keysThen([[AppType.Standard, 1], [AppType.Relationship, 2]], AppType.Standard);
  console.log(keys.length);
}
