// expect-error: method `Picker.pick` does not return a value on all paths
// expect-error: getter `Picker.first` does not return a value on all paths
// expect-error-count: 2
class Picker {
  private flag: boolean = true;

  pick(flag: boolean): number {
    if (flag) {
      return 1;
    }
  }

  get first(): number {
    if (this.flag) {
      return 1;
    }
  }
}
