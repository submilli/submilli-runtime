export class Widget {
  constructor(readonly id: number) {}
}

export class Top {
  static readonly W: Widget = new Widget(1);

  static pick(w: Widget): Widget {
    return w;
  }
}

export class Mid extends Top {}
