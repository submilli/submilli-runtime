// @target: es2015
class C {
  public(): void {}
  static(): void {}

  public public(): void {}
  public static(): void {}

  public static public(): void {}
  public static static(): void {}
  
  static public(): void {}
  static static(): void {}
}

function main(): void {}
