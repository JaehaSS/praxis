package sample;

public final class Callers {
    private Callers() {}

    public static void callerA() {
        Target.target();
    }

    public static void callerB() {
        Target.target();
    }

    private static void target() {}
}
