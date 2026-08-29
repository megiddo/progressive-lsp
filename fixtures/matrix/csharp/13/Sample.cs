// C# 13: params collections (representative).
public class Sample {
    public int Add(params int[] xs) {
        int s = 0;
        foreach (var x in xs) s += x;
        return s;
    }
}
