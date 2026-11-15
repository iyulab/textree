namespace Textree.Eval;

/// <summary>
/// The normalized tree edit distance (NTED) between two unordered trees: the constrained edit
/// distance of Zhang (1996) — insert, delete and relabel one node at a time, each costing one, with
/// disjoint subtrees mapped to disjoint subtrees — divided by the number of nodes in both trees, so
/// 0 is the same tree and 1 shares nothing. The constrained distance is the polynomial form of the
/// unordered edit distance (the unconstrained one is NP-hard) and never below it. Folders are unlabeled nodes (any folder may become any
/// other at no cost); documents are leaves labeled by which document they are.
/// </summary>
public static class Nted
{
    /// <summary>A node: a folder (<paramref name="Label"/> null) or a document leaf.</summary>
    public sealed record Node(string? Label, IReadOnlyList<Node> Children)
    {
        public static Node Folder(params Node[] children) => new(null, children);

        public static Node Leaf(string label) => new(label, []);

        public int Size => 1 + Children.Sum(c => c.Size);
    }

    /// <summary>A tree of folders with documents as leaves, built from each document's folder path ("" is the root).</summary>
    public static Node TreeOf(IEnumerable<KeyValuePair<string, string>> folderOfDocument)
    {
        var folders = new HashSet<string>(StringComparer.Ordinal) { "" };
        var docs = new Dictionary<string, List<string>>(StringComparer.Ordinal);
        foreach (var (doc, folder) in folderOfDocument)
        {
            for (var f = folder; f.Length > 0; f = Parent(f))
            {
                folders.Add(f);
            }
            (docs.TryGetValue(folder, out var list) ? list : docs[folder] = []).Add(doc);
        }
        Node Build(string f) => new(null,
            folders.Where(x => x.Length > 0 && Parent(x) == f).Order(StringComparer.Ordinal).Select(Build)
                .Concat(docs.GetValueOrDefault(f, []).Order(StringComparer.Ordinal).Select(Node.Leaf))
                .ToList());
        return Build("");
    }

    private static string Parent(string folder)
    {
        var i = folder.LastIndexOf('/');
        return i < 0 ? "" : folder[..i];
    }

    /// <summary>The constrained unordered edit distance between two trees.</summary>
    public static int Distance(Node a, Node b) => new Solver().Tree(a, b);

    /// <summary>The distance divided by the size of both trees (0 = the same tree).</summary>
    public static double Normalized(Node a, Node b) => (double)Distance(a, b) / (a.Size + b.Size);

    private sealed class Solver
    {
        private readonly Dictionary<(Node, Node), int> _tree = new(ReferencePairComparer.Instance);
        private readonly Dictionary<(Node, Node), int> _forest = new(ReferencePairComparer.Instance);

        private static int Relabel(Node a, Node b) =>
            (a.Label is null) == (b.Label is null) && a.Label == b.Label ? 0 : 1;

        private static int ForestSize(Node n) => n.Size - 1;

        public int Tree(Node a, Node b)
        {
            if (_tree.TryGetValue((a, b), out var known))
            {
                return known;
            }
            // Keep a and b mapped to each other: relabel, then map their child forests.
            var best = Forest(a, b) + Relabel(a, b);
            // Delete a, keeping one of its subtrees mapped to b (the rest of a is deleted).
            foreach (var c in a.Children)
            {
                best = Math.Min(best, a.Size - c.Size + Tree(c, b));
            }
            // Insert b, mapping a into one of b's subtrees.
            foreach (var c in b.Children)
            {
                best = Math.Min(best, b.Size - c.Size + Tree(a, c));
            }
            return _tree[(a, b)] = best;
        }

        /// <summary>The distance between the child forests of a and b.</summary>
        private int Forest(Node a, Node b)
        {
            if (_forest.TryGetValue((a, b), out var known))
            {
                return known;
            }
            var ca = a.Children;
            var cb = b.Children;
            int best;
            if (ca.Count == 0 || cb.Count == 0)
            {
                best = ForestSize(a) + ForestSize(b);
            }
            else
            {
                // Match subtrees to subtrees; an unmatched subtree is deleted or inserted whole.
                var n = ca.Count + cb.Count;
                var cost = new int[n, n];
                for (var i = 0; i < n; i++)
                {
                    for (var j = 0; j < n; j++)
                    {
                        cost[i, j] = i < ca.Count && j < cb.Count ? Tree(ca[i], cb[j])
                            : i < ca.Count ? ca[i].Size
                            : j < cb.Count ? cb[j].Size
                            : 0;
                    }
                }
                best = Hungarian.Solve(cost);
                // Or collapse one side: keep a's whole forest inside one subtree of b (b's other
                // nodes inserted), or b's forest inside one subtree of a.
                foreach (var c in cb)
                {
                    best = Math.Min(best, ForestSize(b) - ForestSize(c) + Forest(a, c));
                }
                foreach (var c in ca)
                {
                    best = Math.Min(best, ForestSize(a) - ForestSize(c) + Forest(c, b));
                }
            }
            return _forest[(a, b)] = best;
        }
    }

    private sealed class ReferencePairComparer : IEqualityComparer<(Node, Node)>
    {
        public static readonly ReferencePairComparer Instance = new();

        public bool Equals((Node, Node) x, (Node, Node) y) =>
            ReferenceEquals(x.Item1, y.Item1) && ReferenceEquals(x.Item2, y.Item2);

        public int GetHashCode((Node, Node) p) =>
            HashCode.Combine(System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(p.Item1),
                System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(p.Item2));
    }
}

/// <summary>Minimum-cost perfect assignment on a square matrix (Hungarian method, O(n³)).</summary>
public static class Hungarian
{
    public static int Solve(int[,] cost)
    {
        var n = cost.GetLength(0);
        var u = new long[n + 1];
        var v = new long[n + 1];
        var p = new int[n + 1];
        var way = new int[n + 1];
        for (var i = 1; i <= n; i++)
        {
            p[0] = i;
            var j0 = 0;
            var minv = Enumerable.Repeat(long.MaxValue, n + 1).ToArray();
            var used = new bool[n + 1];
            do
            {
                used[j0] = true;
                int i0 = p[j0], j1 = 0;
                var delta = long.MaxValue;
                for (var j = 1; j <= n; j++)
                {
                    if (used[j])
                    {
                        continue;
                    }
                    var cur = cost[i0 - 1, j - 1] - u[i0] - v[j];
                    if (cur < minv[j])
                    {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if (minv[j] < delta)
                    {
                        delta = minv[j];
                        j1 = j;
                    }
                }
                for (var j = 0; j <= n; j++)
                {
                    if (used[j])
                    {
                        u[p[j]] += delta;
                        v[j] -= delta;
                    }
                    else
                    {
                        minv[j] -= delta;
                    }
                }
                j0 = j1;
            }
            while (p[j0] != 0);
            do
            {
                var j1 = way[j0];
                p[j0] = p[j1];
                j0 = j1;
            }
            while (j0 != 0);
        }
        var total = 0;
        for (var j = 1; j <= n; j++)
        {
            total += cost[p[j] - 1, j - 1];
        }
        return total;
    }
}
