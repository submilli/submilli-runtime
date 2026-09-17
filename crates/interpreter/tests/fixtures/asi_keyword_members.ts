interface Words {
  first: number
  else: number
  catch: number
  finally: number
  extends: number
  implements?: number
}
class Fields {
  first: number = 1
  else: number = 2
  catch: number = 3
  finally: number = 4
  extends: number = 5
  implements: number = 6
}
function main(): void {
  const words: Words = {
    first: 1,
    else: 2,
    catch: 3,
    finally: 4,
    extends: 5,
    implements: 6
  };
  assert(words.else + words.catch + words.finally + words.extends === 14);
  const fields = new Fields();
  assert(fields.implements === 6);
}
