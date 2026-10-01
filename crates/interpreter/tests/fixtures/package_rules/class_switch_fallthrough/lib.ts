// expect-error: no fallthrough
// expect-error-count: 1
class Router {
  route(code: number): void {
    switch (code) {
      case 1: {
        console.log("one");
      }
      case 2:
        break;
    }
  }
}
