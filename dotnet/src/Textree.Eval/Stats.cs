namespace Textree.Eval;

/// <summary>A measured proportion: successes out of trials, with its Wilson 95% interval.</summary>
public sealed record Proportion(double Value, double Low, double High, int Successes, int N)
{
    public static Proportion Of(int successes, int n)
    {
        if (n == 0)
        {
            return new Proportion(double.NaN, double.NaN, double.NaN, 0, 0);
        }
        const double z = 1.959963984540054;
        var p = (double)successes / n;
        var denom = 1 + z * z / n;
        var centre = (p + z * z / (2 * n)) / denom;
        var half = z * Math.Sqrt(p * (1 - p) / n + z * z / (4.0 * n * n)) / denom;
        return new Proportion(Round(p), Round(centre - half), Round(centre + half), successes, n);
    }

    /// <summary>Whether this interval lies wholly above <paramref name="other"/>'s.</summary>
    public bool Above(Proportion other) => Low > other.High;

    private static double Round(double v) => Math.Round(v, 4);
}

/// <summary>A measured mean of scores in [0, 1], with a normal-approximation 95% interval.</summary>
public sealed record Mean(double Value, double Low, double High, int N)
{
    public static Mean Of(IReadOnlyList<double> scores)
    {
        if (scores.Count == 0)
        {
            return new Mean(double.NaN, double.NaN, double.NaN, 0);
        }
        var m = scores.Average();
        var sd = scores.Count > 1 ? Math.Sqrt(scores.Sum(s => (s - m) * (s - m)) / (scores.Count - 1)) : 0;
        var half = 1.96 * sd / Math.Sqrt(scores.Count);
        return new Mean(Math.Round(m, 4), Math.Round(m - half, 4), Math.Round(m + half, 4), scores.Count);
    }

    public bool Above(Mean other) => Low > other.High;
}

public static class Median
{
    public static double Of(IEnumerable<double> values)
    {
        var v = values.OrderBy(x => x).ToList();
        if (v.Count == 0)
        {
            return double.NaN;
        }
        return v.Count % 2 == 1 ? v[v.Count / 2] : (v[v.Count / 2 - 1] + v[v.Count / 2]) / 2;
    }
}
