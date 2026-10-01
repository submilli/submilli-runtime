// expect-error: unreachable code
// expect-error-count: 1
class Reader {
  read(): number {
    return 1;
    console.log("after return");
  }
}
